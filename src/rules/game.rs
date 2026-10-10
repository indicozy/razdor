use std::collections::BTreeSet;
use std::sync::Arc;

use crate::dt::dtm::Scenario;

use super::ai::{self, AiNews, AiStats, Beaten};
use super::battle::{Battle, Outcome, Team};
use super::clock::{Clock, Tick};
use super::content::{Content, HeroClass, ItemId, Source, Stat, UnitId, WageKind};
use super::economy::VillageOffer;
use super::events::{ArmyId, EventEngine, EventOutcome};
use super::fog::{self, Fog};
use super::formation::Slot;
use super::items::{self, EquipError};
use super::journal::History;
use super::magic::{self, ActiveSpell};
use super::save::ScenarioRef;
use super::ships::Ship;
use super::map::{is_water, step_minutes, Tile, TileMap};
use super::rng::{EventRng, Rng, WORLD_MUSIC_DRAW};
use super::units::{PromoteError, Unit};
use super::world::{Army, LocationKind, Stationed, Troop, World, AI_BUDGET_CAP};

fn wide_row_default() -> bool {
    true
}

/// Real seconds each hero step and each wait tick plays over: the original's
/// `WalkDelay = 150 + (100 − WalkSpeed) × 2.5` ms at the shipped `WalkSpeed=100` (world.md
/// §2). Game time per real second follows from the step's own minutes.
pub const STEP_SECONDS: f32 = 0.15;
/// Game minutes of a wait tick (world.md §6): waiting 1 h is 2 ticks, 4 h 8 ticks.
pub const WAIT_TICK_MINUTES: f32 = 30.0;
/// The hero's speed by class (world.md §2): knight and archmage 5, ranger 4 (his steps
/// take 80% of the time).
pub const KNIGHT_SPEED: u32 = 5;
pub const RANGER_SPEED: u32 = 4;
/// The demo's gangs chase the player inside this many cells (Razdor's own demo rule).
pub const CHASE_RADIUS: i32 = 6;
/// Cells a demo gang's or a ship's pathfinder may expand per search.
const AI_PATH_NODES: usize = 4000;
const SPAWN_EVERY_DAYS: u64 = 3;
const MAX_GANGS_PER_CAMP: usize = 2;
/// Unworn items the hero's backpack holds: 256 slots, shown as a scrolling grid 5 wide.
pub const PACK_SIZE: usize = 256;
/// Spells the book holds (the original's message comes at 15).
pub const SPELL_BOOK_SIZE: usize = 15;
/// Items on sale in each demo market after a restock.
pub const MARKET_STOCK: usize = 6;
/// Percent chance that a beaten demo gang drops an item.
const GANG_LOOT_CHANCE: i32 = 30;
/// Percent chance that a demo village pays tribute with an item instead of gold.
pub(crate) const TRIBUTE_ITEM_CHANCE: i32 = 25;

#[derive(Debug, PartialEq, Eq)]
pub enum HireError {
    NotOffered,
    /// Not enough gold (or mana, for units paid in mana).
    NotEnoughGold,
    SquadFull,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Currency {
    Gold,
    Mana,
}

/// A price in gold or, for `Nature=Elemental` units (Community Update), in mana.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Price {
    pub amount: i32,
    pub currency: Currency,
}

impl Price {
    pub fn gold(amount: i32) -> Price {
        Price { amount, currency: Currency::Gold }
    }

    /// `amount` in the currency unit type `unit` is paid in.
    pub fn for_unit(content: &Content, unit: UnitId, amount: i32) -> Price {
        let currency = if content.paid_in_mana(unit) { Currency::Mana } else { Currency::Gold };
        Price { amount, currency }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum TradeError {
    NoMarket,
    NotEnoughGold,
    NoSuchItem,
    /// Personal items cannot be sold.
    NotForSale,
}

/// What a village paid.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Tribute {
    Gold(i32),
    Item(ItemId),
}

#[derive(Debug, PartialEq, Eq)]
pub enum BattleResult {
    /// `loot` went into the pack; `left_behind` items did not fit.
    /// `level_ups`: (squad index, new level). `captured`: the castle or fort now the player's.
    Victory {
        reward: i32,
        /// Mana from surrendered enemies ("they pray for you").
        mana: i32,
        lost: usize,
        loot: Vec<ItemId>,
        left_behind: usize,
        level_ups: Vec<(usize, i32)>,
        captured: Option<usize>,
    },
    /// Nobody won: no XP (the original pays it only for a victory).
    Withdrew { lost: usize },
    Defeat,
}

/// The noon report (video notes: the daily report comes at 12:00): money and mana after
/// the day's income and wages.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DayReport {
    /// Absolute day index (see [`Clock::day_index`]).
    pub day: u64,
    /// The nominal income of the player's towns, castles and forts ×F/100 (not what the
    /// noon pays: economy.md §9).
    pub income: i32,
    /// The gold wage bill without Rear Service, and the mana paid to elementals.
    pub wages: i32,
    pub mana_wages: i32,
    /// Units that could not be paid: they sit out battles until paid.
    pub unpaid: usize,
    /// Units that left after going unpaid for `MaxTimeNotUpkeep`.
    pub deserted: Vec<UnitId>,
    /// Gold and mana when the report opened, before the payment.
    pub gold: i32,
    pub mana_total: i32,
}

/// A place an event showed on the map: its centre and the cells that were dark before.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shown {
    pub at: Tile,
    pub cells: Vec<Tile>,
    /// The scenario event that showed it: the interface flies there once that event's window
    /// is closed (the original queues the glides at its OK).
    pub event: Option<u16>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Event {
    /// The party stopped on a location (index into `world.locations`).
    Arrived(usize),
    /// A hostile army caught the party (index into `world.armies`).
    Encounter(usize),
    /// A friendly army met the party on the road; no battle.
    Met(usize),
    /// The party walked into a hostile castle or fort that had no garrison: it is his.
    Captured(usize),
    NewDay(DayReport),
    /// Something the scenario's event engine did: a message, a question, a quest, the end.
    /// World effects are already applied.
    Script(EventOutcome),
    /// AI armies fought within the hero's sight, or one took or besieged his building
    /// (`rules::ai`).
    Battle(AiNews),
    /// A squad member reached a new level outside battle (scenario XP): (squad index, level).
    LevelUp(usize, i32),
    /// Entering village `at`, the hero took its tribute (economy.md §3: on entering, all the
    /// gold and mana, no button): what it paid, and the mana.
    Tribute { at: usize, paid: Tribute, mana: i32 },
    /// A spell read on the map landed, or was lost (`Game::begin_cast`).
    SpellCast { spell: u32, target: magic::CastTarget, outcome: magic::CastOutcome },
    /// A scenario event cast `spell` on the hero's army (0x4ab1ec): its effect plays over him
    /// and his walk ends on this cell; no window opens.
    EventSpell { spell: u32 },
}

/// Who the next battle is against.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Foe {
    /// The garrison of a location (a castle, a fort, ruins, a demo camp).
    Garrison(usize),
    Army(usize),
}

/// The whole game state. It is saved with serde (`rules::save`), except the content and the
/// statics of the world and the event engine, which a load rebuilds from the scenario.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Game {
    #[serde(skip)]
    pub content: Arc<Content>,
    /// Squad member 0 is always the hero.
    pub squad: Vec<Unit>,
    pub gold: i32,
    pub mana: i32,
    pub clock: Clock,
    /// Party position in world units (see `map::center`).
    pub pos: (f32, f32),
    /// Remaining route, next tile first.
    pub path: Vec<Tile>,
    pub world: World,
    /// Location the party is standing on, if any.
    pub location: Option<usize>,
    pub foe: Option<Foe>,
    /// Shared bag of unworn items.
    pub pack: Vec<ItemId>,
    /// The hero's spell book (1-based spell indices), cast with [`Game::cast`].
    pub spells: Vec<u8>,
    /// Explored cells (`rules::fog`): off in the demo, on for scenarios.
    pub fog: Fog,
    /// Where the player clicked: the end of the walk under way.
    pub goal: Option<Tile>,
    pub(crate) start_day: u64,
    /// The original's one generator (`rules::rng`): not saved; a map load sets it to 1, a
    /// save load starts it from the load sequence (`rules::save`).
    #[serde(skip)]
    pub(crate) rng: Rng,
    /// The Community event generator (opcode 18), seeded from the clock at every load.
    #[serde(skip)]
    pub(crate) event_rng: EventRng,
    /// A map start or a load has just started the world theme: the ms until its first change
    /// (90 s and the music's draw), for the interface to take ([`Game::take_music_wait`]).
    #[serde(skip)]
    pub(crate) music_wait: Option<u32>,
    pub(crate) battles: u64,
    /// The scenario's event engine (`None` in the demo). Taken out while it runs.
    pub(crate) script: Option<Box<EventEngine>>,
    /// Events produced outside [`Game::tick`] and [`Game::wait`] (at the start, after a
    /// battle, an answer, a rumour); the UI drains them with [`Game::drain_events`].
    pub(crate) pending: Vec<Event>,
    /// Events the engine's effects produced during a run (delays, battles).
    pub(crate) effect_events: Vec<Event>,
    /// Areas revealed by events (lanterns, shown armies): (x, y, radius) in cells, for the
    /// fog of war to take.
    pub pending_reveals: Vec<(i32, i32, i32)>,
    /// Places an event has just shown (lanterns, shown armies) with the cells it uncovered,
    /// for the map to fly to and fade in; the interface takes them. Not saved.
    #[serde(skip)]
    pub shown: Vec<Shown>,
    /// Scenario armies (ids) the player has met / beaten.
    pub(crate) met_armies: BTreeSet<ArmyId>,
    /// The army the player clicked (its `uid`): reaching it always starts a meeting ("click
    /// it to talk or fight"), even if it greeted him before.
    #[serde(default)]
    pub(crate) talk_to: Option<u32>,
    pub(crate) beaten_armies: BTreeSet<ArmyId>,
    /// Scenario armies beaten by AI armies (`rules::ai`).
    #[serde(default)]
    pub(crate) ai_beaten: BTreeSet<ArmyId>,
    /// What the AI did so far (counts).
    #[serde(default)]
    pub ai_stats: AiStats,
    /// Reports of AI battles the player heard of, newest last.
    #[serde(default)]
    pub ai_log: Vec<AiNews>,
    /// 1 knight, 2 archmage, 3 ranger: the class the game started with (events check it).
    pub(crate) archetype: u8,
    /// Army-wide world spells of saves before format 7; a load moves them into the units'
    /// slots (`rules::save`).
    #[serde(default, rename = "effects", skip_serializing)]
    pub(crate) old_effects: Vec<ActiveSpell>,
    /// The game minute the map started (the original's clock 0): `None` in saves before
    /// format 7, which take the start day's midnight.
    #[serde(default)]
    pub(crate) map_start: Option<u64>,
    /// The scenario the game plays (`rules::save`): the demo, or a map file of the install
    /// with a hash of its bytes. The UI sets it for maps ([`Game::set_origin`]).
    pub origin: Option<ScenarioRef>,
    /// An autosave is due (the noon report came): its name. The UI writes it and clears it.
    #[serde(skip)]
    pub autosave_due: Option<String>,
    /// What this campaign map started with from the one before ([`Game::from_campaign`]):
    /// the original's restart snapshot (0x4b5ef8), kept in every save, from which a restart
    /// starts the map again ([`Game::restart`]). `None` on a map started as a new game.
    #[serde(default)]
    pub(crate) carried: Option<Box<super::script::NextMap>>,
    /// The wide front row the game was started with (the original's map header byte 0x121,
    /// from the option at the map's load): a load plays on with it whatever the option says
    /// now (0x4b771c). Saves before format 9 were all wide.
    #[serde(default = "wide_row_default")]
    pub(crate) wide_row: bool,
    /// The hero's ship (`rules::ships`): under him at sea, or parked where he landed.
    #[serde(default)]
    pub ship: Option<Ship>,
    /// A ship was just bought: his planner works on the MIXED map until his next step.
    #[serde(default)]
    pub(crate) ship_bought: bool,
    /// The hero's noon (by its day) waiting for an event scan in which no event fires and
    /// no spell is being read.
    #[serde(default)]
    pub(crate) noon_due: Option<u64>,
    /// The hero's speed as a Community event set it (0xc279e6); `None`: his class's.
    #[serde(default)]
    pub(crate) speed_set: Option<u32>,
    /// The minutes of the hero's next orthogonal step as the original works them out when he
    /// comes onto his cell (0x497c68: the cell's cost × his speed, ×1.5 at the step if it is
    /// diagonal), on the map his at-sea flag chose *before* that arrival updated it: coming
    /// onto the water from land or a building, or starting a map on the water, the cell is
    /// priced on LAND, where water costs 0, so his first step at sea takes no time. `None`:
    /// the cell he stands on, priced now (a save load works it out anew, 0x4b771c).
    #[serde(skip)]
    pub(crate) step_base: Option<u32>,
    /// While the hero steps: the cell he left, which AI armies keep off too.
    #[serde(skip)]
    pub(crate) step_from: Option<Tile>,
    /// The hero's step flag (0x75e0c7): the walk timer clears it every frame (0x4ae71e) and
    /// sets it in the frame a step ends and the next begins (0x4ae975); nothing else writes
    /// it. So it stays set after a walk that ran its course or was stopped at a step's end,
    /// through the waits that follow, until the next walk: an AI army's attack or greeting
    /// counts only while it is set (0x4ade3c). Clear before the first walk and after a
    /// step stopped before it began (0x4ad94c).
    #[serde(default)]
    pub(crate) step_flag: bool,
    /// The building defence the hero's unit strengths were last counted with (his record's
    /// +0x378c at the last recount 0x4a16d4, which writes each unit's cached strength
    /// +0x1ae): an AI army scoring him copies those strengths (ai.md §4), so after he walks
    /// out of his town they still count its defence until the next recount
    /// ([`Game::recount_hero`]). A save from before this field reads 0.
    #[serde(default)]
    pub(crate) hero_strength_bd: i32,
    /// The battle under way: the enemy's troop records its fighters came from, in their
    /// order (its living troops at the start), for [`Game::battle_write_back`].
    #[serde(skip)]
    pub(crate) battle_troops: Vec<usize>,
    /// The offset of the hero's last step: the original keeps his direction after a walk
    /// (0x75c050), and AI armies keep off the cell it points to (0x4a399c). `None` before
    /// his first step.
    #[serde(default)]
    pub(crate) facing: Option<(i32, i32)>,
    /// The first day whose 12:00 is the hero's noon: the day after the start, then the day
    /// after each noon paid (0x4a41d8: the next noon is the day after the moment it was
    /// paid). `None` in older saves: the day after the start.
    #[serde(default)]
    pub(crate) noon_from: Option<u64>,
    /// The building clicked and the one he stood in at the last click (a pursuit plans
    /// with them).
    #[serde(skip)]
    pub(crate) click_buildings: (Option<usize>, Option<usize>),
    /// The building he walked into while an event's window opened, entered once the windows
    /// are read (0x4ed42c; [`Game::enter_waiting_building`]).
    #[serde(skip)]
    pub(crate) waiting_entry: Option<usize>,
    /// A heal, a raise or a trade in the building window asks for the events to be checked
    /// when the window closes (0x4ed440, set at 0x4b13cb, 0x4b14dc and 0x4b9fd9, tested at
    /// 0x4b8f63); a check that fires nothing clears it (0x4ac3a6).
    #[serde(default)]
    pub(crate) scan_on_close: bool,
    /// The name the player gave the hero (`#HERONAME`); `None`: his class's name.
    #[serde(default)]
    pub hero_name: Option<String>,
    /// What the player learnt, with dates (`rules::journal`); empty in older saves.
    #[serde(default)]
    pub journal: History,
    /// The offer of the village the hero stands in, made on entering (`rules::economy`).
    #[serde(default)]
    pub(crate) offer: Option<(usize, VillageOffer)>,
    /// The offer's `Random(5)`, drawn as its window is built (0x4aca80): the blessing's spell
    /// `3 + 2·r`, the witch's mana `300 + 50·r`.
    #[serde(default)]
    pub(crate) offer_roll: i32,
    /// The village that made the last offer (cleared when its tribute is taken) and what
    /// was offered.
    #[serde(default)]
    pub(crate) offered_at: Option<usize>,
    #[serde(default)]
    pub(crate) last_offer: Option<VillageOffer>,
    /// The player's stored income (+0x16e4): his castles' and forts' `income` plus the
    /// village stocks of his last noon (economy.md §1 step 5). Rear Service and the
    /// innkeeper read it.
    #[serde(default)]
    pub(crate) stored_income: i32,
    /// The Community mana-short flag (0xc25eeb): up when the player's mana is 0 or below at
    /// any army's noon, down only at a short-gold noon (economy.md §1 steps 8–10).
    #[serde(default)]
    pub(crate) mana_short: bool,
    /// Real seconds into the step (or wait tick) under way ([`STEP_SECONDS`] each).
    #[serde(skip)]
    pub(crate) step_elapsed: f32,
    /// Wait ticks still to play in real time ([`Game::begin_wait`], or a reading).
    #[serde(skip)]
    pub(crate) wait_ticks: u32,
    /// The Community endless wait (F4, 0xc277d2): the ticks it has played so far, while it
    /// runs ([`Game::begin_endless_wait`]).
    #[serde(skip)]
    pub(crate) endless_wait: Option<u32>,
    /// The spell being read while the wait ticks play ([`Game::begin_cast`]).
    #[serde(skip)]
    pub(crate) reading: Option<magic::Reading>,
    /// Spells events cast while a spell was being read, landing after it
    /// ([`Game::end_reading`]).
    #[serde(skip)]
    pub(crate) queued_casts: Vec<u32>,
    /// Real seconds since the world last moved, for drawing armies between cells.
    #[serde(skip)]
    pub(crate) since_step: f32,
    /// The hero's last step for drawing: from and to (map points), played over the same
    /// window as the armies' walks of that step ([`Game::display_pos`]).
    #[serde(skip)]
    pub(crate) hero_glide: Option<((f32, f32), (f32, f32))>,
    /// Stretches of time so far (a hero's step, a wait tick): which window an event came in.
    #[serde(skip)]
    pub(crate) stretches: u64,
    /// The events of a stretch whose window still plays on screen ([`Game::tick_shown`]).
    #[serde(skip)]
    pub(crate) held: Vec<(u64, Event)>,
    /// The places events showed in such a stretch: their flights wait with the windows.
    #[serde(skip)]
    pub(crate) held_shown: Vec<(u64, Shown)>,
    /// Game minutes of the last stretch (a hero's step or a wait tick), for drawing: the
    /// armies' walk frames follow the game time inside it ([`Game::army_walk_frame`]).
    #[serde(skip)]
    pub(crate) stretch_minutes: f32,
    /// The hero stopped: the armies' snap and its idle draws are due ([`Game::armies_snap`]),
    /// after the windows the stop opened have drawn their chords.
    #[serde(skip)]
    pub(crate) snap_due: bool,
    /// The stop under way has had its snap already (an AI army's attack or greeting) or has
    /// none (a run into an army or a garrison, 0x4ad94c in the middle of the step).
    #[serde(skip)]
    pub(crate) snapped: bool,
    /// "Improved enemy AI in battle" (the original's `OptValue9`, "expert" in Razdor's
    /// settings): the player's choice, set by the interface, not part of the save.
    #[serde(skip)]
    pub improved_ai: bool,
    /// Reports of AI battles to hand to the interface with the slice's events.
    #[serde(skip)]
    pub(crate) ai_events: Vec<Event>,
    /// The AI's simulated battles already played (`rules::ai`).
    #[serde(skip)]
    pub(crate) sims: std::cell::RefCell<ai::SimCache>,
}

/// What an AI army's step needs of the hero: the cells it may not enter (his logical cell
/// and the one ahead of him: the one he steps to), and where he is for the AI: his logical
/// cell, the cell he leaves while a step is under way (world.md §5).
pub(crate) struct HeroCells {
    pub(crate) cells: [Option<Tile>; 2],
    pub(crate) at: Tile,
    /// The frame where his step ends (his step flag 0x75e0c7 set): only an arrival then
    /// attacks or greets him (0x4ade3c).
    pub(crate) boundary: bool,
}

/// Talk counter an army towards the hero is set to after a greeting (world.md §4.3).
pub(crate) const TALKED: i32 = -500;

/// A demo gang walks its path while its banked minutes cover the next step (world.md §5,
/// 0x4a399c): `cost(the cell it leaves) × speed`, ×1.5 diagonally; `cost` gives the cost units
/// of a cell, `None` where it cannot go (the route is dropped). A step onto one of the hero's
/// cells spends its time but the gang stays put. Every step taken (or tried) marks it
/// arrived for [`Game::ai_contact`]. Remembers where it stood for drawing. (The scenario's
/// armies walk by the AI's step clock, `rules::ai`.)
fn step_army(map: &TileMap, a: &mut Army, cost: &dyn Fn(Tile) -> Option<u16>, hero: &HeroCells) {
    while let Some(&next) = a.path.first() {
        if cost(next).is_none() {
            a.path.clear();
            break;
        }
        let here = a.tile(map);
        let left = cost(here).unwrap_or(0);
        let need = step_minutes(map.grid, here, next, left, a.speed.max(1));
        if a.budget < need {
            break;
        }
        a.budget -= need;
        a.arrived = true;
        if hero.cells.contains(&Some(next)) {
            break;
        }
        a.pos = map.center(next);
        a.path.remove(0);
        a.walk.points.push(a.pos);
        a.walk.minutes.push(need);
    }
}

/// What the cell ahead does to the hero's step ([`Game::step_contact`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StepContact {
    /// Army `i` is engaged.
    Army(usize),
    /// The garrison of building `l` is engaged.
    Garrison(usize),
    /// Building `l` is taken on the way.
    Captured(usize),
}

impl Game {
    fn with_world(content: Arc<Content>, world: World, squad: Vec<Unit>, tile: Tile) -> Game {
        let clock = world.start;
        let wide_row = content.formation == super::formation::Formation::WIDE;
        let mut g = Game {
            squad,
            gold: 0,
            mana: 0,
            content,
            clock,
            pos: world.map.center(tile),
            path: Vec::new(),
            location: world.location_at(tile),
            world,
            foe: None,
            pack: Vec::new(),
            spells: Vec::new(),
            fog: Fog::disabled(0, 0),
            goal: None,
            start_day: clock.day_index(),
            rng: Rng::map_load(),
            event_rng: EventRng::from_clock(),
            music_wait: None,
            battles: 0,
            script: None,
            pending: Vec::new(),
            effect_events: Vec::new(),
            pending_reveals: Vec::new(),
            shown: Vec::new(),
            met_armies: BTreeSet::new(),
            talk_to: None,
            beaten_armies: BTreeSet::new(),
            ai_beaten: BTreeSet::new(),
            ai_stats: AiStats::default(),
            ai_log: Vec::new(),
            archetype: 1,
            old_effects: Vec::new(),
            map_start: Some(clock.total_minutes() as u64),
            origin: None,
            autosave_due: None,
            carried: None,
            wide_row,
            ship: None,
            ship_bought: false,
            noon_due: None,
            speed_set: None,
            step_base: None,
            step_from: None,
            step_flag: false,
            hero_strength_bd: 0,
            battle_troops: Vec::new(),
            facing: None,
            noon_from: None,
            click_buildings: (None, None),
            waiting_entry: None,
            scan_on_close: false,
            hero_name: None,
            journal: History::default(),
            offer: None,
            offer_roll: 0,
            offered_at: None,
            last_offer: None,
            stored_income: 0,
            mana_short: false,
            step_elapsed: 0.0,
            wait_ticks: 0,
            endless_wait: None,
            reading: None,
            queued_casts: Vec::new(),
            since_step: 0.0,
            hero_glide: None,
            stretches: 0,
            held: Vec::new(),
            held_shown: Vec::new(),
            stretch_minutes: 0.0,
            snap_due: false,
            snapped: false,
            improved_ai: false,
            ai_events: Vec::new(),
            sims: Default::default(),
        };
        // The hero draws no wage; everyone counts as paid at the start.
        let now = clock.total_minutes() as u64;
        g.squad[0].wage_kind = WageKind::Leader;
        g.squad.iter_mut().for_each(|u| u.last_paid = now);
        // The scenario garrisons of the player's own buildings are his troops there (never
        // paid).
        let c = g.content.clone();
        for l in g.world.locations.iter_mut().filter(|l| l.owned() && !l.garrison.is_empty()) {
            let troops = std::mem::take(&mut l.garrison);
            l.stationed.extend(troops.iter().map(|t| Stationed { unit: troop_unit(&c, t) }));
        }
        // The map load's draws (engine.md §3.2): the state is 1, the markets are stocked,
        // then the world music draws its first change time.
        g.restock_markets();
        g.music_wait = Some(90_000 + g.rng.random(WORLD_MUSIC_DRAW) as u32);
        g.fog = Fog::disabled(g.world.map.w, g.world.map.h);
        g
    }

    /// A new demo game. `content` must hold the demo units (see [`World::standard`]).
    pub fn new(content: Arc<Content>, hero: HeroClass) -> Self {
        let world = World::standard(&content);
        let home = world.locations[0].tile;
        let id = hero.unit();
        let slot = content.formation.new_unit_slot(&[]).expect("empty formation");
        let squad = vec![Unit::new(&content, id, slot)];
        let gold = content.start_gold(hero);
        let mut g = Game::with_world(content, world, squad, home);
        g.gold = gold;
        g.spells = g.content.start_spells(hero);
        g.archetype = archetype_of(hero);
        g.origin = Some(ScenarioRef::Demo);
        g
    }

    /// Records which map file the game plays, for saves.
    pub fn set_origin(&mut self, origin: ScenarioRef) {
        self.origin = Some(origin);
    }

    /// A new game on an original scenario, with the hero preset of `hero`.
    pub fn from_scenario(content: Arc<Content>, scenario: &Scenario, hero: HeroClass) -> Self {
        let mut g = Game::unstarted(content, scenario, hero);
        g.start_script();
        g
    }

    /// The scenario's opening events (they wait in [`Game::drain_events`]).
    pub(crate) fn start_script(&mut self) {
        let opening = self.run_script();
        self.pending.extend(opening);
    }

    /// A game on a scenario whose opening events have not run yet.
    pub(crate) fn unstarted(content: Arc<Content>, scenario: &Scenario, hero: HeroClass) -> Self {
        let mut world = World::from_scenario(scenario, &content);
        let start = world.hero_start(scenario, &content, hero);
        for &l in &start.owned {
            world.give_to_player(l);
        }
        let mut leader = Unit::new(&content, hero.unit(), start.hero_slot);
        leader.heal_full(&content);
        let mut squad = vec![leader];
        squad.extend(start.troops.iter().map(|t| troop_unit(&content, t)));
        let mut g = Game::with_world(content, world, squad, start.tile);
        g.arrange_at_load();
        // The class's speed is set first (0x4b4300: 0x68dcd8, copied to the hero's +0x1694),
        // then the map load puts him on his cell (0x4b5913 → 0x497c68), before he is at sea:
        // his first step is priced at his class's speed, on LAND, 0 on the water.
        g.archetype = archetype_of(hero);
        g.step_base = Some(g.land_step_base(start.tile));
        // A preset on the water ("Тихая пристань") puts him there, at sea: aboard a ship
        // *(guess: the original plans on its MIXED map while he is on water)*.
        if g.world.is_sea(start.tile) {
            g.ship = Some(Ship { tile: start.tile, aboard: true });
        }
        g.fog = fog::for_scenario(&g.world.map, Some(scenario), true);
        g.look_around();
        g.gold = start.gold;
        g.mana = start.mana;
        g.pack = start.items;
        g.spells = start.spells;
        g.archetype = archetype_of(hero);
        g.script = Some(Box::new(EventEngine::new(scenario)));
        g.ai_init(false);
        // The AI's set-up adds the player's castles' and forts' income to his stored income
        // too (0x4a1ff0). The original seems to add it again on top of a saved value after a
        // load (economy.md, Unknowns); Razdor adds it at the map's start only.
        g.stored_income = g.world.locations.iter().filter(|l| l.kind.capturable() && l.owned()).map(|l| l.gold_income).sum();
        // The map load ends with his army recounted on his cell (0x4b5b64 → 0x497240(0, 1)).
        g.recount_hero();
        g
    }

    /// The hero's army recount (0x4a16d4(0), most often through 0x497240(0, 1)): his units'
    /// cached strengths take the defence of his record now (+0x378c: the building he stands
    /// in when it is his, else 0, 0x497c68). The original runs it at the map load, at every
    /// event window closed (0x4ab1ec), at his noon (0x4abfbc), in a building window
    /// (0x4ba854: the hire and garrison tabs and the close; the hire 0x4bd3a4), after his
    /// battles and when his army window opens (0x4d1814); not when he walks.
    pub(crate) fn recount_hero(&mut self) {
        let here = self.location.or_else(|| self.world.location_covering(self.tile()));
        self.hero_strength_bd = here.map(|l| &self.world.locations[l]).filter(|l| l.owned()).map_or(0, |l| l.garrison_defence.max(0));
    }

    /// The army window opened (0x4d1814): it recounts his army.
    pub fn army_window_opened(&mut self) {
        self.recount_hero();
    }

    pub fn hero(&self) -> &Unit {
        &self.squad[0]
    }

    /// Switches the front row's width of a game under way (a Razdor setting, applied at once
    /// rather than only to new games): the game fights in `formation` from its next battle on,
    /// and its saves record the new width. The hero's units keep their cells when the new shape
    /// has them; a unit on a cell it lacks (the two edge cells of a 6-wide row going to 4) moves
    /// to a free cell, its own row first ([`Formation::free_slot`]). AI armies are arranged when
    /// their battle starts, so they need nothing. Not during a battle.
    pub fn set_formation(&mut self, formation: super::formation::Formation) {
        if self.content.formation == formation || self.foe.is_some() {
            return;
        }
        self.content = Arc::new(self.content.with_formation(formation));
        self.wide_row = formation == super::formation::Formation::WIDE;
        let mut taken: Vec<Slot> = self.squad.iter().map(|u| u.slot).filter(|s| formation.contains(*s)).collect();
        for u in self.squad.iter_mut() {
            if !formation.contains(u.slot) {
                if let Some(s) = formation.free_slot(&taken, u.slot.row) {
                    u.slot = s;
                    taken.push(s);
                }
            }
        }
    }

    /// The map load puts every army, the hero's too, through a battle side and back (0x4b2504
    /// → 0x49855c, 0x4988c0): the side is auto-arranged (483b3c, [`Battle::auto_arrange`])
    /// and its grid becomes the army's formation. So the hero's starting army stands as the
    /// auto-arrange puts it, not where adding the units put it (reserve first, 0x495ce0).
    /// His building defence is still 0 then (he enters his cell after, 0x497c68). A campaign
    /// map's carried-over army brings its own formation back after it (0x4b5b64).
    /// The map's armies and every garrison go through the same round trip (saves-data.md
    /// §10.1 step 9), so a castle's archers do not start in front where adding put them.
    fn arrange_at_load(&mut self) {
        let player: Vec<(usize, &Unit)> = self.squad.iter().enumerate().filter(|(_, u)| u.alive()).collect();
        for (i, s) in arranged(&self.content, &player, 0) {
            self.squad[i].slot = s;
        }
        // Every army record, those waiting off the map too (0x4b56a8 loops over all of them).
        let content = self.content.clone();
        let w = &mut self.world;
        let armies = w.armies.iter_mut().chain(w.inactive.iter_mut()).chain(w.respawns.iter_mut().map(|r| &mut r.army));
        let troops = armies.map(|a| &mut a.troops).chain(w.locations.iter_mut().map(|l| &mut l.garrison));
        for troops in troops {
            arrange_troops(&content, troops, 0);
        }
    }

    /// The hero's name for `#HERONAME`: the one the player chose, else his class's name.
    pub fn hero_name(&self) -> String {
        match &self.hero_name {
            Some(n) if !n.trim().is_empty() => n.trim().to_string(),
            _ => self.hero().name(&self.content).to_string(),
        }
    }

    /// Names the hero (an empty name means his class's name).
    pub fn set_hero_name(&mut self, name: &str) {
        let n = name.trim();
        self.hero_name = (!n.is_empty()).then(|| n.to_string());
    }

    pub fn hero_class(&self) -> Option<HeroClass> {
        HeroClass::of_unit(self.hero().def)
    }

    /// Army cap: the formation's size.
    pub fn max_squad(&self) -> usize {
        self.content.formation.capacity()
    }

    pub fn can_afford(&self, p: Price) -> bool {
        match p.currency {
            Currency::Gold => self.gold >= p.amount,
            Currency::Mana => self.mana >= p.amount,
        }
    }

    /// Pays `p` if the party has enough.
    pub(crate) fn spend(&mut self, p: Price) -> bool {
        if !self.can_afford(p) {
            return false;
        }
        match p.currency {
            Currency::Gold => self.gold -= p.amount,
            Currency::Mana => self.mana -= p.amount,
        }
        true
    }

    pub fn tile(&self) -> Tile {
        self.world.map.tile_at(self.pos)
    }

    pub fn moving(&self) -> bool {
        !self.path.is_empty()
    }

    /// The game minute the map started: the original's clock counts from it.
    pub(crate) fn map_start(&self) -> u64 {
        self.map_start.unwrap_or(self.start_day * super::clock::MINUTES_PER_DAY)
    }

    /// The class the game started with: the hero's sight, speed and cast divisor stay those
    /// of the starting class whatever unit an event makes him (world.md §2.1, 0x4b4300).
    pub fn start_class(&self) -> HeroClass {
        match self.archetype {
            2 => HeroClass::Archmage,
            3 => HeroClass::Ranger,
            _ => HeroClass::Knight,
        }
    }

    /// The hero's speed: minutes per cost unit of an orthogonal step ([`KNIGHT_SPEED`],
    /// [`RANGER_SPEED`] by the starting class), or what a Community speed event set
    /// (0xc279e6).
    pub fn hero_speed(&self) -> u32 {
        if let Some(s) = self.speed_set {
            return s;
        }
        if self.start_class() == HeroClass::Ranger {
            RANGER_SPEED
        } else {
            KNIGHT_SPEED
        }
    }

    /// Game minutes of the hero's step from `from` onto its neighbour `to` (world.md §2.1):
    /// the cost of the cell he leaves on LAND, or on MIXED at sea, times his speed, ×1.5
    /// diagonally.
    pub fn step_time(&self, from: Tile, to: Tile) -> f32 {
        let w = &self.world;
        let left = if self.aboard() { w.mixed_cost(from) } else { w.map.cost(from).unwrap_or(0) };
        step_minutes(w.map.grid, from, to, left, self.hero_speed())
    }

    /// Minutes of the hero's step from `from` onto `to` as he takes it: the base the original
    /// set when he came onto `from` ([`Game::step_base`]), ×1.5 diagonally.
    fn hero_step_minutes(&self, from: Tile, to: Tile) -> f32 {
        match self.step_base {
            Some(base) => step_minutes(self.world.map.grid, from, to, 1, base),
            None => self.step_time(from, to),
        }
    }

    /// The base of his next step on LAND (cost × speed): 0 on the water.
    pub(crate) fn land_step_base(&self, t: Tile) -> u32 {
        u32::from(self.world.map.cost(t).unwrap_or(0)) * self.hero_speed()
    }

    /// Minutes the hero needs to walk `path`.
    pub fn travel_minutes(&self, path: &[Tile]) -> f32 {
        self.world.map.path_minutes_by(self.tile(), path, &|a, b| self.step_time(a, b)) as f32
    }

    /// Minutes left on the current route.
    pub fn minutes_left(&self) -> f32 {
        self.travel_minutes(&self.path)
    }

    /// Unit types the barracks here offers now (the player's or a friendly town, castle,
    /// fort or church, with stock left).
    pub fn recruits_here(&self) -> Vec<UnitId> {
        match self.location.map(|l| &self.world.locations[l]) {
            Some(l) if l.hires(&self.content) => l.recruits.iter().filter(|r| r.stock != Some(0)).map(|r| r.unit).collect(),
            _ => Vec::new(),
        }
    }

    /// Squad member `unit` goes to cell `slot` of the formation (the army screen and the
    /// barracks, by dragging); a unit standing there takes its old cell, as at a battle's
    /// deployment. False if `slot` is not a cell of the formation.
    pub fn move_unit(&mut self, unit: usize, slot: Slot) -> bool {
        if unit >= self.squad.len() || !self.content.formation.slots().any(|s| s == slot) {
            return false;
        }
        let from = self.squad[unit].slot;
        if let Some(other) = self.squad.iter().position(|u| u.slot == slot) {
            self.squad[other].slot = from;
        }
        self.squad[unit].slot = slot;
        true
    }

    /// The pursuit of the army clicked (the original's "automatically pursue the chosen
    /// army", 0x4aedd1): when it has stepped since the hero's last step, his route is
    /// planned again to its new cell; if that cell is closed or out of reach the pursuit
    /// ends and the hero stops where he is. Returns false when he stopped.
    fn follow_army(&mut self) -> bool {
        let Some(uid) = self.talk_to else { return true };
        let map = &self.world.map;
        let Some(a) = self.world.armies.iter().find(|a| a.uid == uid) else {
            self.talk_to = None;
            return true;
        };
        let at = a.tile(map);
        if !a.arrived || self.goal == Some(at) {
            return true;
        }
        let path = self.plan_from(self.tile(), at, self.click_buildings, Some(uid), false);
        if path.is_empty() {
            self.talk_to = None;
            self.path.clear();
            self.goal = None;
            return false;
        }
        self.path = path;
        self.goal = Some(at);
        true
    }

    /// The route a click on `to` would walk (world.md §1.3): the first click shows it.
    /// Empty if `to` is not a target ([`Game::can_target`]) or out of reach.
    pub fn route_to(&self, to: Tile) -> Vec<Tile> {
        self.plan(to)
    }

    /// A click on `t` is a target (world.md §1.3, 0x4cbf20): an explored cell open on the
    /// hero's planner map, or his parked ship; at sea not a bridge. Anything else does
    /// nothing.
    pub fn can_target(&self, t: Tile) -> bool {
        let w = &self.world;
        if !w.map.in_bounds(t) || !self.fog.explored(t) {
            return false;
        }
        if self.aboard() && w.location_covering(t).is_some_and(|l| w.locations[l].kind.is_bridge()) {
            return false;
        }
        self.planner_cost(t) != 0 || self.parked_ship() == Some(t)
    }

    /// Walk to `to` along the planned route (the second click, world.md §1.3). A click on an
    /// army makes it the chased army, unless it stands in a building other than a bridge.
    /// Returns false if it can't be reached; as in the original (0x4cc99f) the walk to cell
    /// (0, 0) never starts, though its route is shown.
    pub fn set_destination(&mut self, to: Tile) -> bool {
        let path = self.plan(to);
        if path.is_empty() || to == (0, 0) {
            return false;
        }
        let w = &self.world;
        let in_building = w.location_at(to).is_some();
        self.talk_to = w.armies.iter().find(|a| a.tile(&w.map) == to).filter(|_| !in_building).map(|a| a.uid);
        self.click_buildings = self.buildings_of_click(to);
        self.path = path;
        self.goal = Some(to);
        self.wait_ticks = 0;
        self.endless_wait = None;
        self.reading = None;
        true
    }

    /// The route a click on `to` walks now (world.md §1.2–1.3): the original's flood from
    /// the clicked cell ([`TileMap::flood_route`]) on the hero's planner map, with these cells
    /// closed: castles and forts whose attitude to him is 0 or less and ruins not his
    /// (unless it is the building clicked or the one he stands in); at sea, when he stands on
    /// a bridge or clicked land, every bridge; every unexplored cell; the cells of the
    /// stationary guards and of the armies with a meeting event waiting that stand next to
    /// him (both except the army clicked; 0x4cc583, 0x4cc601). Moving armies and other
    /// buildings are crossed. A click on the parked ship costs 1 there for the plan.
    pub fn plan(&self, to: Tile) -> Vec<Tile> {
        if !self.can_target(to) {
            return Vec::new();
        }
        let w = &self.world;
        let clicked = w.armies.iter().find(|a| a.tile(&w.map) == to).map(|a| a.uid);
        self.plan_from(self.tile(), to, self.buildings_of_click(to), clicked, true)
    }

    /// The building under a click on `to` and the one the hero stands in: the planner leaves
    /// both open.
    fn buildings_of_click(&self, to: Tile) -> (Option<usize>, Option<usize>) {
        (self.world.location_covering(to), self.world.location_covering(self.tile()))
    }

    /// [`Game::plan`] from `from`, `(target, standing)` the buildings left open; `reopen`:
    /// the hero's own cell is reopened before the fog (the click does so, the pursuit does
    /// not, and keeps the buildings of the original click: 0x4aedd1); `clicked`: the army
    /// clicked or chased, which the army mask leaves open.
    fn plan_from(&self, from: Tile, to: Tile, (target, standing): (Option<usize>, Option<usize>), clicked: Option<u32>, reopen: bool) -> Vec<Tile> {
        let w = &self.world;
        let map = &w.map;
        if map.mask_index(to).is_none() {
            return Vec::new();
        }
        let ship_click = self.parked_ship() == Some(to);
        let cost = |t: Tile| if ship_click && t == to { 1 } else { self.planner_cost(t) };
        let at_sea = self.aboard();
        let bridges = at_sea && (standing.is_some_and(|l| w.locations[l].kind.is_bridge()) || !is_water(map.surface(to)));
        let mut mask = vec![1u16; (map.w * map.h).max(0) as usize];
        // The armies (0x4cc583 and 0x4cc601 at a click, 0x4aee3e and 0x4aeec3 in the
        // pursuit): an army on the map (+0x16a1) other than the one clicked or chased closes
        // its own cell when it is a stationary guard (patrol flag +0x16bb, radius +0x16bc 0),
        // or when a meeting event waits for it (+0x3826) and it stands next to the hero
        // (0x4826f8 distance 1, any of the 8 neighbours). Friend or foe alike; every other
        // army, a moving one included, can be walked through (its contact comes on the step,
        // `Game::step_contact`).
        for a in &w.armies {
            if Some(a.uid) == clicked {
                continue;
            }
            let t = a.tile(map);
            let closed = ai::stationary(a)
                || (map.distance(t, from) == 1 && self.script.as_deref().is_some_and(|e| e.meeting_waiting(self, a.id)));
            if let Some(i) = map.mask_index(t).filter(|_| closed) {
                mask[i] = 0;
            }
        }
        for (l, loc) in w.locations.iter().enumerate() {
            if Some(l) == target || Some(l) == standing {
                continue;
            }
            let closed = loc.bars_hero() || (bridges && loc.kind.is_bridge());
            if closed {
                for i in loc.cells().filter_map(|t| map.mask_index(t)) {
                    mask[i] = 0;
                }
            }
        }
        if let Some(i) = map.mask_index(from).filter(|_| reopen) {
            mask[i] = 1;
        }
        let open = |t: Tile| map.mask_index(t).map_or(0, |i| if self.fog.explored(t) { mask[i] } else { 0 });
        // The pursuit gives up on a target cell closed once the fog is laid over the mask
        // (0x4aedd1): an army gone into the dark is not followed. A click is not tested (the
        // seed only needs a cost; the dark is no target anyway).
        if !reopen && open(to) == 0 {
            return Vec::new();
        }
        map.flood_route(&cost, &open, &[(to, 0)], from).map(|r| r.0).unwrap_or_default()
    }

    /// How far the hero sees, in cells: 9 for the knight, 8 for the archmage, 10 for the
    /// ranger (world.md §3), by the starting class.
    pub fn sight_radius(&self) -> i32 {
        fog::sight_radius(self.start_class())
    }

    /// Reveals the hero's surroundings. Returns true if new ground came into view.
    pub fn look_around(&mut self) -> bool {
        let r = self.sight_radius();
        self.fog.reveal(self.tile().0, self.tile().1, r)
    }

    /// Lights a lantern: reveals radius `r` (cells) around cell `(x, y)`.
    pub fn reveal(&mut self, x: i32, y: i32, r: i32) {
        self.fog.reveal(x, y, r);
    }

    pub fn stop(&mut self) {
        self.path.clear();
        self.goal = None;
        self.talk_to = None;
        self.wait_ticks = 0;
        self.endless_wait = None;
        self.reading = None;
        self.step_elapsed = 0.0;
    }

    /// A left click or any key while the hero walks (interface.md §7.3, 0x4cd132): the route
    /// is cut to end at the cell of the step under way, so he finishes that step and stops
    /// there, arriving as at any walk's end. Only the route is cut, as in the original: the
    /// army he chases stays his target. A wait is not cut.
    pub fn cut_walk(&mut self) {
        self.path.truncate(1);
    }

    /// A left click or a key press during a wait of 1 or 4 hours or the endless wait *(a
    /// Razdor choice the players asked for: the original's waits run to their end whatever
    /// is clicked or pressed, checked under Wine on РК1, 2026-10-04)*: the half-hour tick
    /// under way plays out and the wait ends after it, as a walk's stop ends after the step
    /// under way. A reading is not cut.
    pub fn cut_wait(&mut self) {
        if !self.waiting() {
            return;
        }
        self.endless_wait = None;
        self.wait_ticks = self.wait_ticks.min(1);
    }

    /// The world theme's first change time if a map start or a load has just drawn it
    /// (interface.md §13); taken once.
    pub fn take_music_wait(&mut self) -> Option<u32> {
        self.music_wait.take()
    }

    /// The map music's change when it is due, drawn from the game's generator as the
    /// original draws it (`rules::music::rotate`): the new pick and the ms to the next one.
    pub fn music_rotate(&mut self, last: usize) -> (usize, u32) {
        super::music::rotate(last, &mut self.rng)
    }

    /// Which of the three `Global-Event` chords an event window, the village window or the
    /// shipyard window plays as it opens: `Random(3)` of the game's generator (interface.md
    /// §14), so opening one shifts every later roll.
    pub fn event_chord(&mut self) -> usize {
        self.rng.random(3) as usize
    }

    /// Waits in real time (the UI's 1 h and 4 h): `hours × 2` wait ticks of
    /// [`WAIT_TICK_MINUTES`], one every [`STEP_SECONDS`], played by [`Game::tick`].
    pub fn begin_wait(&mut self, hours: u32) {
        if self.foe.is_some() {
            return;
        }
        self.path.clear();
        self.goal = None;
        self.reading = None;
        self.endless_wait = None;
        self.wait_ticks = hours * 2;
    }

    /// The Community endless wait (F4 on the idle map, 0xc277d2): the wait of one tick is
    /// started with its end test switched off (0xc27802), so the ticks go on until
    /// [`Game::end_endless_wait`]; an event's dialog opens without ending it (0xc2782b).
    pub fn begin_endless_wait(&mut self) {
        if self.foe.is_some() {
            return;
        }
        self.begin_wait(0);
        self.wait_ticks = 1;
        self.endless_wait = Some(0);
    }

    /// F5 during the endless wait (0xc27802): the end test is back, and as the wait asked
    /// for one tick it stops at once when a tick has played, else after its first. The
    /// minutes of the tick under way are dropped *(the original keeps the part already
    /// shown; Razdor counts game time per whole tick)*.
    pub fn end_endless_wait(&mut self) {
        if let Some(done) = self.endless_wait.take() {
            if done >= 1 {
                self.wait_ticks = 0;
                self.step_elapsed = 0.0;
                self.snap_due = true;
            }
        }
    }

    /// The Community endless wait (F4) is under way.
    pub fn endless_waiting(&self) -> bool {
        self.endless_wait.is_some() && self.wait_ticks > 0
    }

    /// A real-time rest is under way (not a reading: [`Game::reading`]).
    pub fn waiting(&self) -> bool {
        self.wait_ticks > 0 && self.reading.is_none()
    }

    /// Where to draw the hero: his last step plays over the window after it was taken, the
    /// same window the armies' steps of that time play in ([`Game::army_display_pos`]), so
    /// the hero and the armies move together as in the original (world.md §2.2).
    pub fn display_pos(&self) -> (f32, f32) {
        match self.hero_glide {
            // Moved otherwise since (a battle, an event, a ship): drawn where he is.
            Some((a, b)) if b == self.pos && self.since_step < STEP_SECONDS => {
                let k = (self.since_step / STEP_SECONDS).clamp(0.0, 1.0);
                (a.0 + (b.0 - a.0) * k, a.1 + (b.1 - a.1) * k)
            }
            _ => self.pos,
        }
    }

    /// Where the hero is heading on screen: the cell of the step being drawn, else the next
    /// of his route.
    pub fn display_heading(&self) -> Option<(f32, f32)> {
        match self.hero_glide {
            Some((_, b)) if b == self.pos && self.since_step < STEP_SECONDS => Some(b),
            _ => self.path.first().map(|&t| self.world.map.center(t)),
        }
    }

    /// The last step's window is still playing on screen: what it brought (a battle, the
    /// windows of its events) waits for its end, as the original's come at the step's end.
    pub fn step_playing(&self) -> bool {
        self.since_step < STEP_SECONDS && (self.hero_glide.is_some_and(|(_, b)| b == self.pos) || self.world.armies.iter().any(|a| !a.walk.minutes.is_empty() && a.walk.points.last() == Some(&a.pos)))
    }

    /// Where to draw army `a`: along the steps it took in the last step or wait tick, played
    /// over that window's real time as the original does ([`Walk`]).
    pub fn army_display_pos(&self, a: &Army) -> (f32, f32) {
        // Moved otherwise since (a battle, a respawn, an event): drawn where it is.
        if a.walk.points.last() != Some(&a.pos) {
            return a.pos;
        }
        a.walk.at(self.since_step / STEP_SECONDS).unwrap_or(a.pos)
    }

    /// The walk frame of army `a`'s figure (engine.md §7: the original's AI walk frames run
    /// by game time, 0x4ad660 → 0x4ad314): frames 3–6, the next every 10 game minutes, while
    /// it has a step to take; `None` (its standing frame) without one. Game time only flows
    /// while the hero walks or waits, so the frames stand still with it; inside a stretch it
    /// runs smoothly, as the clock does between the original's ticks.
    pub fn army_walk_frame(&self, a: &Army) -> Option<u32> {
        if a.path.is_empty() {
            return None;
        }
        let k = (self.since_step / STEP_SECONDS).clamp(0.0, 1.0) as f64;
        let minutes = self.clock.total_minutes() - (1.0 - k) * self.stretch_minutes as f64;
        let tens = (minutes / 10.0).floor() as i64;
        Some(3 + tens.rem_euclid(4) as u32)
    }

    /// [`Game::tick`] for the screen: the events of a step or wait tick come once its window
    /// has played (the hero's and the armies' steps drawn to their cells), as the original's
    /// come at the end of the step; an attacking army is seen arriving. The rules' order is
    /// the same; only what the player sees waits.
    pub fn tick_shown(&mut self, real_dt: f32) -> Vec<Event> {
        let before = self.shown.len();
        let events = self.tick(real_dt);
        let now = self.stretches;
        self.held.extend(events.into_iter().map(|e| (now, e)));
        // A lantern or a shown army of the stretch waits too: its flight comes at its
        // event's OK (0x4ab1ec), so its window must be up first.
        let shown: Vec<Shown> = self.shown.drain(before..).collect();
        self.held_shown.extend(shown.into_iter().map(|s| (now, s)));
        let playing = self.step_playing();
        let (out, keep): (Vec<_>, Vec<_>) = std::mem::take(&mut self.held).into_iter().partition(|&(w, _)| w < now || !playing);
        self.held = keep;
        let (due, keep): (Vec<_>, Vec<_>) = std::mem::take(&mut self.held_shown).into_iter().partition(|&(w, _)| w < now || !playing);
        self.held_shown = keep;
        self.shown.extend(due.into_iter().map(|(_, s)| s));
        out.into_iter().map(|(_, e)| e).collect()
    }

    /// [`Game::tick_shown`] still holds events of a step being drawn: their windows are not
    /// on screen yet, though the rules already ran them (a victory event among them), or the
    /// places their lanterns showed.
    pub fn holds_events(&self) -> bool {
        !self.held.is_empty() || !self.held_shown.is_empty()
    }

    /// Advance the world by `real_dt` seconds (world.md §2): each hero step and each wait
    /// tick plays over [`STEP_SECONDS`]; the game time a step takes is its own cost. Time only
    /// flows while the party walks or waits.
    pub fn tick(&mut self, real_dt: f32) -> Vec<Event> {
        let mut events = Vec::new();
        self.since_step += real_dt;
        if self.foe.is_some() || (!self.moving() && self.wait_ticks == 0) {
            self.step_elapsed = 0.0;
            if self.foe.is_some() {
                self.end_reading(&mut events);
                self.wait_ticks = 0;
                self.endless_wait = None;
            }
            return events;
        }
        self.step_elapsed += real_dt;
        // A snap still due from the last stop comes before the world moves again.
        self.armies_snap();
        let (mut walked, mut waited) = (false, false);
        self.snapped = false;
        while self.step_elapsed >= STEP_SECONDS && (self.moving() || self.wait_ticks > 0) {
            self.step_elapsed -= STEP_SECONDS;
            let stepping = self.moving();
            let go = if stepping {
                walked = true;
                self.hero_step(&mut events)
            } else {
                waited = true;
                match self.endless_wait.as_mut() {
                    // The endless wait's end test is off: its one tick never runs out.
                    Some(done) => *done += 1,
                    None => self.wait_ticks -= 1,
                }
                let from = events.len();
                let mut go = self.wait_tick(&mut events);
                // The noon report opens in the event window and the scan counts it as an
                // event that fired (0x4abfbc): it ends the wait (0x4ae42f → 0xc2782b), not the
                // endless one.
                if self.endless_wait.is_none() && events[from..].iter().any(|e| matches!(e, Event::NewDay(_))) {
                    go = false;
                }
                // The reading is done, an event fired (the original pops the wait off its
                // queue, 0x4ae4f2, and the spell lands at once), or an event set a battle
                // over his book.
                if self.wait_ticks == 0 || self.foe.is_some() || events[from..].iter().any(fires) {
                    self.end_reading(&mut events);
                }
                go
            };
            if !go || events.iter().any(Event::needs_reading) {
                // An event's window opened by the step's scan ends the walk on this cell
                // (0x4aed41 → 0x4ae5d8), which counts as arriving on it again; the building
                // he clicked waits for the windows to be read if he stands in it (0x4aed64).
                if go && stepping {
                    self.stop_for_reading();
                }
                // Stop and read: time stands still while a message is open. A reading
                // goes on after it.
                self.path.clear();
                // The endless wait goes on under the event's dialog (0xc2782b), not after a
                // battle.
                if self.reading.is_none() && (self.endless_wait.is_none() || self.foe.is_some()) {
                    self.wait_ticks = 0;
                }
                break;
            }
        }
        // The hero stopped: the end of a walk (0x4ae5d8, then 0x4ad8a0 at the frame's end),
        // an event or an AI army's attack or greeting stopping it (0x4ade3c), or the end of a
        // wait, also one an event cut short (0x4ae24c, 0x4ae42f); not a run into an army or a
        // garrison in the middle of a step (0x4ad94c snaps them without the idle draws), nor
        // the endless wait going on under an event's dialog (0xc2782b).
        if (walked && !self.moving() && !self.snapped) || (waited && self.wait_ticks == 0 && !self.snapped) {
            self.snap_due = true;
        }
        if !self.moving() && self.wait_ticks == 0 {
            self.step_elapsed = 0.0;
            self.endless_wait = None;
        }
        events
    }

    /// The hero takes the next step of his route (world.md §2.2, §4.2): the cell he is about
    /// to enter is checked first ([`Game::step_contact`]): an army there or a building's
    /// guard or garrison stops him where he is; an unguarded village, or an empty castle,
    /// fort or ruins ill-disposed to him, is taken on the way; at sea, land ahead ends the
    /// route on it. Then he moves, charged the cell he leaves, and the world moves on by the
    /// step's time; an AI army that stepped next to him may attack or greet him
    /// ([`Game::ai_contact`]). At the end of the route he stops, and the building he stands
    /// in (not a bridge or an obelisk) opens. Returns false when the walk ended.
    fn hero_step(&mut self, events: &mut Vec<Event>) -> bool {
        if !self.follow_army() {
            return false;
        }
        let Some(&next) = self.path.first() else { return false };
        let from = self.tile();
        // The walk timer's frame clears the step flag; the step sets it again as it ends.
        self.step_flag = false;
        match self.step_contact(next) {
            Some(StepContact::Army(i)) => {
                self.snapped = true;
                self.path.clear();
                self.goal = None;
                self.talk_to = None;
                // The original engages any army stepped onto, friendly or not (world.md §4.2).
                let e = Event::Encounter(i);
                self.foe = Some(Foe::Army(i));
                self.engage(e, events);
                return false;
            }
            Some(StepContact::Garrison(l)) => {
                self.snapped = true;
                self.path.clear();
                self.goal = None;
                self.talk_to = None;
                self.foe = Some(Foe::Garrison(l));
                events.push(Event::Arrived(l));
                return false;
            }
            Some(StepContact::Captured(l)) => {
                if !self.world.locations[l].owned() {
                    self.world.give_to_player(l);
                    events.push(Event::Captured(l));
                }
            }
            None => {}
        }
        let minutes = self.hero_step_minutes(from, next);
        if self.landing(next) {
            // He walks onto the land and stops there; the ship waits on the water he left.
            self.path.truncate(1);
            self.land(from);
        }
        self.path.remove(0);
        let was = self.pos;
        self.pos = self.world.map.center(next);
        self.move_to_cell(from, next);
        self.look_around();
        // His facing stays the step's direction until the next one (0x4ae8e0).
        self.facing = Some((next.0 - from.0, next.1 - from.1));
        self.pass_time_walking(minutes, from, events);
        // His step plays over the window its time opened (an event that moved him meanwhile
        // drops it: drawn where he is).
        self.hero_glide = Some((was, self.world.map.center(next)));
        if let Some(e) = self.ai_contact() {
            let attack = matches!(e, Event::Encounter(_));
            // The stop snaps the armies (0x4ad8a0) before an attack's events run, after a
            // greeting's and before their window opens (0x4ade3c).
            if attack {
                self.snap_now();
            }
            // The events run with the army as the met army; a greeting stops the walk only
            // when one of them fired (0x4ade3c), an attack always.
            if self.meet(e, events) || attack {
                if !attack {
                    self.snap_now();
                }
                self.path.clear();
                self.goal = None;
                self.talk_to = None;
                return false;
            }
        }
        if self.path.is_empty() {
            self.arrive_at_end(events);
            return false;
        }
        true
    }

    /// An event's window cut the walk short (0x4aed41): it ends on his cell as a walk's end
    /// does (0x4ae5d8, the arrival repeated, so a building he is on is entered), and the
    /// building of the clicked cell, if he stands in it, waits to be entered once the windows
    /// are read (0x4aed64 → 0x4ed42c, [`Game::enter_waiting_building`]).
    fn stop_for_reading(&mut self) {
        self.goal = None;
        self.talk_to = None;
        let here = self.tile();
        self.move_to_cell(here, here);
        if let Some(l) = self.location.filter(|&l| self.click_buildings.0 == Some(l)) {
            self.waiting_entry = Some(l);
        }
    }

    /// The end of a walk (0x4ae5d8): he stops on his cell, which counts as arriving on it
    /// again, so a building he stands in is entered ([`Game::enter_building`]). When the
    /// step's event scan opened an event's window (0x4aed3a), the building is not entered
    /// now: if it is the one he clicked (0x4ed430), it waits for the windows to be read
    /// (0x4ed42c, [`Game::enter_waiting_building`]); else it is not entered at all.
    fn arrive_at_end(&mut self, events: &mut Vec<Event>) {
        self.goal = None;
        self.talk_to = None;
        let here = self.tile();
        let before = self.location;
        self.move_to_cell(here, here);
        let Some(l) = self.location else { return };
        if self.world.locations[l].kind == LocationKind::Obelisk {
            return;
        }
        if events.iter().any(Event::needs_reading) {
            if self.click_buildings.0 == Some(l) {
                self.waiting_entry = Some(l);
            }
            return;
        }
        self.enter_building(l, before != Some(l), events);
    }

    /// The building he walked into while an event's window was open, once the windows are
    /// read (the event's OK, 0x4c206c → 0x4ab1ec: with no chained event, the building waiting
    /// at 0x4ed42c is entered, 0x4bbc84). The interface calls it when its last dialog closes;
    /// not while a fight is pending, nor when he no longer stands there.
    pub fn enter_waiting_building(&mut self) -> Vec<Event> {
        let mut events = Vec::new();
        if self.foe.is_some() {
            return events;
        }
        if let Some(l) = self.waiting_entry.take() {
            if self.location == Some(l) {
                self.enter_building(l, false, &mut events);
            }
        }
        events
    }

    /// A click on the building he stands in enters it again (0x4cd0aa → 0x4bbc84): a
    /// village rolls its offer and pays what has refilled since, as on arrival.
    pub fn reenter_building(&mut self) -> Vec<Event> {
        let mut events = Vec::new();
        if let Some(l) = self.location {
            self.enter_building(l, false, &mut events);
        }
        events
    }

    /// Entering building `l` (0x4bbc84): its window opens unless it is a bridge or an
    /// obelisk ([`Event::Arrived`]), with a village's offer or tribute. `scan` runs the
    /// events first when he has only now come into it; one that opens its window keeps the
    /// building waiting for it (0x4ed42c).
    fn enter_building(&mut self, l: usize, scan: bool, events: &mut Vec<Event>) {
        if scan {
            // Entered only now: its local events.
            let script = self.run_script();
            let shown = script.iter().any(Event::needs_reading);
            events.extend(script);
            if shown {
                self.waiting_entry = Some(l);
                return;
            }
        }
        self.visit_village(l);
        // The building's window recomputes his army and marks his pairs with the AI's
        // armies to be rescored on its hire, garrison and close tabs (0x4ba854 →
        // 0x497240(0, 1)); time stands while it is open, so here is as good.
        self.mark_dirty(ai::HERO);
        self.recount_hero();
        events.push(Event::Arrived(l));
        events.extend(self.auto_tribute(l));
    }

    /// What stops or changes the hero's step onto `next` (world.md §4.2, 0x4ad94c), in this
    /// order:
    /// 1. An army stands there: it is engaged, friendly or not, unless the cell belongs to a
    ///    town, tavern, church, smithy, shipyard, altar or dungeon, or the army is well
    ///    disposed to him (attitude above 0) and the cell is a building other than a bridge.
    /// 2. A cell of a village, castle, fort, ruins or bridge: the last army on the map
    ///    standing in it (+0x3788, the building it stands in, not its home) guards it, and is
    ///    engaged when ill-disposed to him (0 or less) or on a bridge.
    /// 3. Unguarded: a castle or fort whose attitude is 0 or less, or ruins, with an empty
    ///    garrison is taken (else the garrison is engaged); a village is taken whatever its
    ///    owner, also when the route only crosses it.
    fn step_contact(&self, next: Tile) -> Option<StepContact> {
        use LocationKind as K;
        let w = &self.world;
        let map = &w.map;
        let building = w.location_covering(next);
        let kind = building.map(|l| w.locations[l].kind);
        if let Some(i) = w.armies.iter().position(|a| a.tile(map) == next) {
            let sheltered = matches!(kind, Some(K::Town | K::Tavern | K::Church | K::Smithy | K::Shipyard | K::Altar | K::Entrance));
            let welcome = w.armies[i].attitude > 0 && kind.is_some_and(|k| !k.is_bridge());
            if !sheltered && !welcome {
                return Some(StepContact::Army(i));
            }
        }
        let l = building?;
        let loc = &w.locations[l];
        if !matches!(loc.kind, K::Village | K::Castle | K::Fort | K::Ruins | K::Camp) && !loc.kind.is_bridge() {
            return None;
        }
        if let Some(g) = w.armies.iter().rposition(|a| a.mind.standing == Some(l)) {
            if w.armies[g].attitude <= 0 || loc.kind.is_bridge() {
                return Some(StepContact::Army(g));
            }
            return None;
        }
        match loc.kind {
            K::Village => Some(StepContact::Captured(l)),
            K::Castle | K::Fort | K::Ruins | K::Camp if loc.attitude <= 0 || loc.kind == K::Ruins => {
                if loc.cleared || loc.garrison.is_empty() {
                    (loc.kind != K::Camp).then_some(StepContact::Captured(l))
                } else {
                    Some(StepContact::Garrison(l))
                }
            }
            _ => None,
        }
    }

    /// One wait tick: [`WAIT_TICK_MINUTES`] pass, armies move. AI armies never attack or
    /// greet the hero while he waits (world.md §4.3). Returns false when it ended.
    fn wait_tick(&mut self, events: &mut Vec<Event>) -> bool {
        self.pass_time(WAIT_TICK_MINUTES, events);
        // An AI army's attack or greeting while he waits, his step flag still set from his
        // last walk (0x4ade3c): as after a step, the stop snaps the armies, the events run
        // with the army met, and a greeting stops the wait only when one of them fired.
        if let Some(e) = self.ai_contact() {
            let attack = matches!(e, Event::Encounter(_));
            if attack {
                self.snap_now();
            }
            if self.meet(e, events) || attack {
                if !attack {
                    self.snap_now();
                }
                return false;
            }
        }
        !events.iter().any(Event::needs_reading) && self.foe.is_none()
    }

    /// Stand still for `hours` at once (1 h = 2 wait ticks, 4 h = 8; world.md §6): time
    /// passes, armies move, the daily moments happen. A message to read ends the wait.
    pub fn wait(&mut self, hours: u32) -> Vec<Event> {
        let mut events = Vec::new();
        if self.foe.is_some() {
            return events;
        }
        self.stop();
        for _ in 0..hours * 2 {
            if !self.wait_tick(&mut events) {
                break;
            }
        }
        self.snap_due = true;
        self.armies_snap();
        events
    }

    /// After the hero's step, what the AI armies' arrivals during it did to him (world.md
    /// §4.3, ai.md §8.1, 0x4ade3c): a greeting wins over any attack, and of several the
    /// last army in order acts (the original's loop keeps overwriting its pick). A greeting
    /// already set both talk counters (`rules::ai`). The demo's gangs attack when they stepped
    /// next to him (|dx| ≤ 1 and |dy| ≤ 1) and he is not in a building other than a bridge or
    /// his own.
    pub(crate) fn ai_contact(&mut self) -> Option<Event> {
        if self.foe.is_some() {
            return None;
        }
        let now = self.clock.total_minutes();
        let contact = |a: &Army, c: ai::Contact| ai::managed(a) && a.mind.contact == Some(c);
        if let Some(i) = self.world.armies.iter().rposition(|a| contact(a, ai::Contact::Greet)) {
            return Some(Event::Met(i));
        }
        let here = self.tile();
        let map = &self.world.map;
        let sheltered = self.world.location_covering(here).is_some_and(|l| {
            let loc = &self.world.locations[l];
            !loc.kind.is_bridge() && !loc.owned()
        });
        let gang = |a: &Army| {
            let t = a.tile(map);
            !ai::managed(a) && !sheltered && a.arrived && (t.0 - here.0).abs() <= 1 && (t.1 - here.1).abs() <= 1 && a.hostile()
        };
        let i = self.world.armies.iter().rposition(|a| (contact(a, ai::Contact::Attack) || gang(a)) && now >= a.ignore_until)?;
        self.foe = Some(Foe::Army(i));
        Some(Event::Encounter(i))
    }

    /// Game time passes, in slices of at most one wait tick: the day's moments (00:00 and
    /// 12:00, world.md §6), spells run out, armies move with the minutes banked, the
    /// scenario's events run.
    pub(crate) fn pass_time(&mut self, minutes: f32, events: &mut Vec<Event>) {
        self.pass_time_as(minutes, WAIT_TICK_MINUTES, events);
    }

    /// [`Game::pass_time`] in slices of at most `slice` minutes: each is a tick of the AI's
    /// step clock (a hero's step is one, however long: the original banks it at once).
    fn pass_time_as(&mut self, minutes: f32, slice: f32, events: &mut Vec<Event>) {
        let mut left = minutes.max(0.0);
        self.stretch_minutes = left;
        // The new stretch plays from the start of a window; only a step of his own glides
        // the hero ([`Game::hero_step`] sets it after), a wait tick does not replay the last.
        self.since_step = 0.0;
        self.hero_glide = None;
        self.stretches += 1;
        // A new stretch for drawing: the steps of this time play in the next window.
        for a in &mut self.world.armies {
            a.walk.points.clear();
            a.walk.points.push(a.pos);
            a.walk.minutes.clear();
            a.walk.banked = (a.budget + left).min(AI_BUDGET_CAP);
            a.arrived = false;
            a.mind.contact = None;
        }
        loop {
            let slice = left.min(slice);
            left -= slice;
            self.pass_slice(slice, events);
            if left <= 0.0 {
                break;
            }
        }
    }

    /// [`Game::pass_time`] for the hero's step from `from`: while it plays, AI armies keep
    /// off both his cells (world.md §5).
    fn pass_time_walking(&mut self, minutes: f32, from: Tile, events: &mut Vec<Event>) {
        self.step_from = Some(from);
        self.pass_time_as(minutes, minutes.max(WAIT_TICK_MINUTES), events);
        self.step_from = None;
        self.step_flag = true;
    }

    /// A slice of time (world.md §6.4): the armies move, then 00:00 comes (0x4a1998 runs at
    /// the end of the AI's advance); the scenario's events run, and the hero's 12:00 comes
    /// in that scan when no event fired and no spell is being read (0x4abfbc), else at a
    /// later scan. His first noon is the day after the start, even after a morning start
    /// (0x4b4388); the AI's armies keep theirs.
    fn pass_slice(&mut self, minutes: f32, events: &mut Vec<Event>) {
        let start = self.clock.total_minutes();
        let ticks = self.clock.advance(minutes as f64);
        self.expire_spells();
        // A midnight comes among the AI's arrivals, at its moment (`Game::ai_move`).
        let midnights: Vec<f64> = ticks.iter().filter_map(|t| match t {
            Tick::Midnight(day) => Some((day * super::clock::MINUTES_PER_DAY) as f64),
            Tick::Noon(_) => None,
        }).collect();
        self.move_armies(minutes, start, &midnights, events);
        for tick in ticks {
            match tick {
                Tick::Midnight(_) => {}
                Tick::Noon(day) => {
                    // AI armies run their noon at their first arrival after it (`rules::ai`).
                    if day >= self.noon_from.unwrap_or(self.start_day + 1) {
                        self.noon_due = Some(day);
                    }
                }
            }
        }
        // Time passed: the scenario's events run.
        let script = self.run_script();
        let fired = script.iter().any(fires);
        events.extend(script);
        if let Some(day) = self.noon_due.filter(|_| !fired && self.reading.is_none()) {
            self.noon_due = None;
            // The next noon is the day after now (0x4a41d8): a noon paid late, after
            // midnight, skips that day's own noon (the original's behaviour).
            self.noon_from = Some(self.clock.day_index() + 1);
            if let Some(report) = self.new_day(day) {
                events.push(Event::NewDay(report));
                // The original autosaves as the noon report opens, named by the date; a noon
                // with no report (no wages, no income) writes none (0x4abfbc).
                self.autosave_due = Some(super::save::date_name(&self.clock));
            }
        }
    }

    /// 00:00 (world.md §6): villages refill (slower as they fill), barracks may gain a unit,
    /// garrisons heal `GarrisonAutoHeal`% — the player's and the AI's.
    pub(crate) fn midnight(&mut self) {
        // Village refill, barracks growth, market redraw and garrison/medic healing
        // (economy.md), then the AI's night (world.md §6).
        self.economy_midnight();
        self.ai_midnight();
    }

    /// The hero's noon (0x4abfbc, economy.md §1): a Ranger heals 15%; with a gold bill or
    /// a nominal income the noon report opens and the payment ([`Game::pay_noon`]) runs when
    /// it is closed, after which a Ranger heals another 20%; else the payment runs at once
    /// and no report is shown. The report shows the gold and mana before the payment, the
    /// nominal income and the bill without Rear Service.
    fn new_day(&mut self, day: u64) -> Option<DayReport> {
        self.ranger_heal(super::economy::RANGER_PERCENT);
        let (income, wages) = (self.daily_income(), self.daily_wages());
        let shown = wages != 0 || income != 0;
        let (gold, mana_total) = (self.gold, self.mana);
        let pay = self.pay_noon();
        if shown {
            self.ranger_heal(super::economy::RANGER_REPORT_PERCENT);
        }
        let n = day.saturating_sub(self.start_day); // the game's first noon is day 1
        if self.world.demo && n.is_multiple_of(SPAWN_EVERY_DAYS) {
            let camps: Vec<_> = self.world.camps().filter(|(_, l)| !l.cleared).map(|(i, l)| (i, l.tile)).collect();
            for (camp, tile) in camps {
                if self.world.armies.iter().filter(|p| p.home == Some(camp)).count() < MAX_GANGS_PER_CAMP {
                    self.world.spawn_gang(camp, tile);
                }
            }
        }
        let super::economy::NoonPay { mana_wages, unpaid, deserted } = pay;
        shown.then_some(DayReport { day, income, wages, mana_wages, unpaid, deserted, gold, mana_total })
    }

    /// Armies move for `minutes` (world.md §2): the AI's armies by its step clock and
    /// arrival rules (`rules::ai`); the demo's gangs bank the minutes (up to
    /// [`AI_BUDGET_CAP`]), chase a nearby hostile hero or patrol, and take the steps they
    /// cover.
    fn move_armies(&mut self, minutes: f32, start: f64, midnights: &[f64], events: &mut Vec<Event>) {
        let now = self.clock.total_minutes();
        let hero_tile = self.tile();
        // The hero's cells: where he stands and, while he steps, the cell he left; standing,
        // the cell ahead of him in the direction of his last step (the original tests his
        // cell plus his direction, which a stop does not clear: 0x4a399c).
        // While he steps his logical cell (the record's, 0x75c064) is still the cell he
        // leaves and the cell ahead of him the one he steps to; the walk timer moves it only
        // in the frame the step ends (0x4ae8cc), so the AI's arrivals of the tick see him
        // there (world.md §5).
        let ahead = self.facing.map(|(dx, dy)| (hero_tile.0 + dx, hero_tile.1 + dy));
        // His step flag: clear during a step (each frame of the walk timer), set in the
        // frame it ends (the walk timer runs before the armies, 0x4ae975); standing or
        // waiting, as his last walk left it.
        let walking = self.step_from.is_some();
        let hero = match self.step_from {
            Some(from) => HeroCells { cells: [Some(from), Some(hero_tile)], at: from, boundary: false },
            None => HeroCells { cells: [Some(hero_tile), ahead], at: hero_tile, boundary: self.step_flag },
        };
        // At the tick's end his step has ended: his cell and the one ahead of him.
        let hero_end = HeroCells { cells: [Some(hero_tile), ahead], at: hero_tile, boundary: walking || self.step_flag };
        let later = self.ai_move(minutes, &hero, &hero_end, start, midnights);
        events.append(&mut self.ai_events);
        let mut armies = std::mem::take(&mut self.world.armies);
        let world = &self.world;
        let map = &world.map;
        for a in armies.iter_mut().filter(|a| !ai::managed(a)) {
            a.budget = (a.budget + minutes).min(AI_BUDGET_CAP);
            let here = a.tile(map);
            let near = a.hostile() && now >= a.ignore_until && map.distance(here, hero_tile) <= CHASE_RADIUS;
            let route = |a: &Army, from: Tile, to: Tile| ai::army_path(world, a, from, to, AI_PATH_NODES);
            if near {
                if !a.chasing || a.path.last() != Some(&hero_tile) {
                    a.path = route(a, here, hero_tile);
                    a.chasing = true;
                }
            } else if a.chasing {
                a.chasing = false;
                a.path.clear();
            }
            if a.path.is_empty() && !a.chasing && a.patrols && a.patrol_radius > 0 && now >= a.rest_until {
                let (r, c) = (a.patrol_radius, a.patrol_centre());
                for _ in 0..3 {
                    let t = (c.0 + self.rng.range(-r, r), c.1 + self.rng.range(-r, r));
                    if map.passable(t) && world.location_at(t).is_none() && map.distance(t, c) <= r {
                        a.path = route(a, here, t);
                        if !a.path.is_empty() {
                            break;
                        }
                    }
                }
                // Rest between patrol legs, or after failing to find one *(guess)*.
                a.rest_until = now + self.rng.range(30, 180) as f64;
            }
            step_army(map, a, &|t| map.cost(t), &hero);
        }
        self.world.armies = armies;
        let end = self.clock.total_minutes();
        for m in later {
            self.clock.set_total_minutes(m);
            self.midnight();
        }
        self.clock.set_total_minutes(end);
    }

    /// Price to hire unit type `kind`: its `Cost` (in mana for elementals).
    pub fn hire_price(&self, kind: UnitId) -> Price {
        Price::for_unit(&self.content, kind, self.content.unit(kind).cost.max(0))
    }

    /// Hire a unit type offered here, at its `Cost`, into the first free cell. The
    /// barracks stock goes down by one.
    pub fn hire(&mut self, kind: UnitId) -> Result<(), HireError> {
        if !self.recruits_here().contains(&kind) {
            return Err(HireError::NotOffered);
        }
        let taken: Vec<Slot> = self.squad.iter().map(|u| u.slot).collect();
        // A new unit takes the first free cell from the reserve forward, whatever it is
        // (495ce0).
        let slot = match self.content.formation.new_unit_slot(&taken) {
            Some(slot) if self.squad.len() < self.max_squad() => slot,
            _ => return Err(HireError::SquadFull),
        };
        if !self.spend(self.hire_price(kind)) {
            return Err(HireError::NotEnoughGold);
        }
        if let Some(l) = self.location {
            if let Some(r) = self.world.locations[l].recruits.iter_mut().find(|r| r.unit == kind) {
                if let Some(n) = r.stock.as_mut() {
                    *n -= 1;
                }
            }
        }
        let mut u = Unit::new(&self.content, kind, slot);
        u.last_paid = self.clock.total_minutes() as u64;
        self.squad.push(u);
        Ok(())
    }

    /// Promote squad member `unit` to class `to` of its upgrade tree; its worn items stay
    /// on ([`Unit::promote`]). The hero cannot be promoted.
    pub fn promote(&mut self, unit: usize, to: UnitId) -> Result<(), PromoteError> {
        let c = self.content.clone();
        if unit == 0 {
            return Err(PromoteError::NotAvailable);
        }
        self.squad.get_mut(unit).ok_or(PromoteError::NotAvailable)?.promote(&c, to)
    }

    /// The defence of the building the hero's army stands in (army +0x378c): one of his own or
    /// of a friend (attitude above 0); 0 elsewhere. It is added to his units' defences in
    /// battle (battle.md §0, 485908) and to their tactical cost.
    pub(crate) fn hero_building_defence(&self) -> i32 {
        let here = self.location.or_else(|| self.world.location_covering(self.tile()));
        here.map(|l| &self.world.locations[l]).filter(|l| l.owned() || l.attitude > 0).map_or(0, |l| l.garrison_defence)
    }

    /// Battle against the pending foe. Unpaid units refuse to fight. Walking into a garrison
    /// makes the player the attacker (the building's extra defence helps the garrison); an
    /// army that catches the player attacks.
    pub fn start_battle(&mut self) -> Battle {
        // A stop's snap still due comes first (0x4ad8a0 runs before the battle opens).
        self.armies_snap();
        // An army fights with its items worn (`ai::army_units`).
        // The beaten army's experience correction scales the player's XP, 0 as it is (no XP,
        // the original's); a garrison's record is cleared and given 100 (4c55b9).
        let correction = match self.foe {
            Some(Foe::Army(i)) => self.world.armies[i].ai.exp_correction,
            _ => 100,
        };
        self.battle_troops = match self.foe {
            Some(Foe::Garrison(l)) => (0..self.world.locations[l].garrison.len()).filter(|&k| self.world.locations[l].garrison[k].alive()).collect(),
            Some(Foe::Army(i)) => (0..self.world.armies[i].troops.len()).filter(|&k| self.world.armies[i].troops[k].alive()).collect(),
            None => Vec::new(),
        };
        let (enemies, attacker, defence) = match self.foe {
            Some(Foe::Garrison(l)) => {
                let loc = &self.world.locations[l];
                (loc.garrison.iter().filter(|t| t.alive()).map(|t| troop_unit(&self.content, t)).collect(), Team::Player, loc.garrison_defence)
            }
            Some(Foe::Army(i)) => (ai::army_units(&self.content, &self.world.armies[i]), Team::Enemy, 0),
            None => (Vec::new(), Team::Player, 0),
        };
        let enemies: Vec<Unit> = enemies;
        // The attacker brings only its paid units, the defender all its living ones (49855c):
        // the player's unpaid units sit out only when he attacks.
        let player: Vec<_> = self.squad.iter().enumerate().filter(|(i, u)| *i == 0 || (u.alive() && (attacker != Team::Player || !u.unpaid))).collect();
        self.battles += 1;
        let mut b = Battle::new(self.content.clone(), &player, &enemies, attacker);
        // In battle every side names its units as 0x49747c does: the hero by his class and
        // name, a named character by his own name (an AI leader or a garrison's too).
        let np = player.len();
        for f in b.fighters.iter_mut().take(np) {
            if let Some(i) = f.squad_index {
                f.name = self.squad_label(i);
            }
        }
        for (f, u) in b.fighters.iter_mut().skip(np).zip(&enemies) {
            f.name = self.unit_label(u);
        }
        if let Some(Foe::Garrison(l)) = self.foe {
            if self.world.locations[l].strengths_bare {
                b.set_bare_strengths(Team::Enemy);
            }
        }
        // The units that stay out still hold their cells in the army's formation.
        let fighting: Vec<usize> = player.iter().map(|p| p.0).collect();
        b.set_bench((0..self.squad.len()).filter(|i| !fighting.contains(i)).map(|i| self.squad[i].slot).collect());
        b.set_xp_correction(correction);
        b.set_improved_ai(self.improved_ai);
        // Lasting world spells are in the units' stats already (their slots).
        // An enemy army attacked in a building of its own side (one hostile to the hero)
        // defends with that building's defence, as a garrison does.
        let army_home = match self.foe {
            Some(Foe::Army(i)) => self.world.location_covering(self.world.armies[i].tile(&self.world.map)).map(|l| &self.world.locations[l]).filter(|l| l.hostile()).map_or(0, |l| l.garrison_defence),
            _ => 0,
        };
        if defence.max(army_home) > 0 {
            b.set_building_defence(Team::Enemy, defence.max(army_home));
        }
        // The hero fighting in a building of his own or of a friend (attitude above 0): its
        // extra defence is added to every defence of his units (battle.md §0, 485908), as for
        // any garrison at home.
        let own = self.hero_building_defence();
        if own > 0 {
            b.set_building_defence(Team::Player, own);
        }
        // The player fights in his army's formation; the enemy is arranged anew (483b3c).
        b.auto_arrange(Team::Enemy);
        self.play_battle_start(&b);
        b
    }

    /// The play log (`diag::play`): both sides of a battle as it starts, every unit with its
    /// stats, bonuses and worn items.
    fn play_battle_start(&self, b: &Battle) {
        let c = &self.content;
        let foe = match self.foe {
            Some(Foe::Army(i)) => self.world.armies.get(i).map(|a| format!("army {} «{}» (items {:?})", a.id, a.name, a.items.iter().map(|&i| c.item(i).name.clone()).collect::<Vec<_>>())),
            Some(Foe::Garrison(l)) => self.world.locations.get(l).map(|l| format!("garrison of building {} «{}»", l.id, l.name)),
            None => None,
        };
        let mut text = format!(
            "BATTLE starts against {} at {:?}; building defence player {} / enemy {}; AI {}",
            foe.unwrap_or_else(|| "?".into()),
            self.tile(),
            b.building_defence(Team::Player),
            b.building_defence(Team::Enemy),
            if self.improved_ai { "expert" } else { "easy" }
        );
        for f in &b.fighters {
            let s = &f.stats;
            let items: Vec<String> = f.items.iter().flatten().map(|&i| c.item(i).name.clone()).collect();
            text.push_str(&format!(
                "\n  {:?} {:?} {} lv{} hp {}/{} AB {} AS {} DB {} DS {} MP {} Ini {} Mnvr {} prot L/E/D {}/{}/{} regen {} vamp {} bonuses {:?} items {:?}",
                f.team, f.slot, f.name, f.level, f.hp, s.max_hp(), s[Stat::AttackBlow], s[Stat::AttackShot], s[Stat::DefenceBlow], s[Stat::DefenceShot], s[Stat::MagicPower],
                s[Stat::Initiative], s[Stat::Manevres], s[Stat::ProtectLife], s[Stat::ProtectElemental], s[Stat::ProtectDeath], s[Stat::Regen], s[Stat::Vampirizm], s.bonuses, items
            ));
        }
        crate::diag::play(&self.clock.label(), &text);
    }

    /// Mana from the enemies' surrender: when every remaining enemy has `Surrender > 0`
    /// the side gives up, and those units' values pray for the victor (48bfb4); units killed
    /// before give none. In the footage a fort garrison of one `Surrender=20` unit gave 20.
    fn surrender_mana(&self, battle: &Battle) -> i32 {
        battle.surrender_mana(Team::Player)
    }

    /// Writes the battle back into the squad: HP, deployed cells, XP and levels. The dead
    /// (except the hero, who survives while his army does) stay in the army as corpses until
    /// resurrected or buried; the dead hold no items, so theirs go to the pack. Potion effects
    /// end. A won garrison fight captures a castle or fort (owner = player, its income counts
    /// at once, and one day of it is paid as the prize, as in the footage) and gives ruins'
    /// treasure; a beaten army leaves the map and pays [`Game::victory_gold`] and its items.
    /// Surrendered enemies give mana.
    /// Then the scenario's events run (an army beaten); what they do waits in
    /// [`Game::drain_events`].
    pub fn resolve_battle(&mut self, battle: &Battle) -> BattleResult {
        crate::diag::play(&self.clock.label(), &format!("BATTLE log:\n  {}\nBATTLE ends: {:?} after {} turns", battle.log.join("\n  "), battle.outcome(), battle.round));
        let result = self.settle_battle(battle);
        // A building taken from its garrison is not entered: he fought it from the cell
        // before it and stays there, outside it (the original's entered building 0x68dc74
        // stays none, no window opens); a click on it walks him in (world.md §7.2, checked
        // under Wine on РК1's ruins).
        let after = self.run_script();
        self.pending.extend(after);
        result
    }

    /// The battle under way written back into both armies, as the original does after every
    /// action of the player's battle (0x4c4f8c after the player's, 0x4c57bc after each of the
    /// enemy's: 0x48bb10 copies the sides out, 0x4988c0 writes them into the army records):
    /// every unit that fights has the HP it has now, 0 when it fell. The interface calls it
    /// after each action; the battle's end writes the rest ([`Game::resolve_battle`]).
    pub fn battle_write_back(&mut self, battle: &Battle) {
        let now = self.clock.total_minutes() as u64;
        let c = self.content.clone();
        let mut enemy = 0;
        for f in &battle.fighters {
            match (f.team, f.squad_index) {
                (Team::Player, Some(i)) => {
                    if let Some(u) = self.squad.get_mut(i) {
                        u.hp = f.hp.max(0);
                    }
                }
                (Team::Enemy, _) => {
                    let k = self.battle_troops.get(enemy).copied();
                    enemy += 1;
                    let troop = match (self.foe, k) {
                        (Some(Foe::Army(a)), Some(k)) => self.world.armies.get_mut(a).and_then(|a| a.troops.get_mut(k)),
                        (Some(Foe::Garrison(l)), Some(k)) => self.world.locations.get_mut(l).and_then(|l| l.garrison.get_mut(k)),
                        _ => None,
                    };
                    if let Some(t) = troop {
                        super::ai::write_hp(&c, t, f.hp, now);
                    }
                }
                _ => {}
            }
        }
    }

    fn settle_battle(&mut self, battle: &Battle) -> BattleResult {
        // The battle's end puts him on his cell again (0x4c50ec → 0x497c68): his next step is
        // priced there as it stands.
        self.step_base = None;
        // The army's formation is rebuilt from the battle grid (4988c0): the survivors keep
        // the cells they ended on (a cell outside the formation is lost); those left without
        // one, the units that did not fight first, then the dead, take free cells, reserve
        // first, columns in plain order (not the preferred order of a new unit).
        let formation = self.content.formation;
        let mut placed = vec![false; self.squad.len()];
        let mut dead = vec![false; self.squad.len()];
        let mut taken: Vec<Slot> = Vec::new();
        for r in battle.player_results() {
            let u = &mut self.squad[r.squad_index];
            u.hp = r.hp;
            // The hero's 1 HP comes after the formation is rebuilt (4906a0).
            let alive = battle.fighters.iter().any(|f| f.squad_index == Some(r.squad_index) && f.alive());
            dead[r.squad_index] = !alive;
            if alive && formation.contains(r.slot) && !taken.contains(&r.slot) {
                u.slot = r.slot;
                taken.push(r.slot);
                placed[r.squad_index] = true;
            }
        }
        let dead_now = |i: usize| dead[i] || !self.squad[i].alive();
        let rest: Vec<usize> = (0..self.squad.len()).filter(|&i| !placed[i] && !dead_now(i)).chain((0..self.squad.len()).filter(|&i| !placed[i] && dead_now(i))).collect();
        for i in rest {
            if let Some(s) = formation.after_battle_slot(&taken) {
                self.squad[i].slot = s;
                taken.push(s);
            }
        }
        // The potions end with every battle of the player (0x4c50ec → 0x490720), then each
        // unit is rebuilt: its HP follows its maximum. Only then is the XP paid, and the
        // rebuild after it rescales the HP again for a new level (experience.md §2).
        let c = self.content.clone();
        for u in &mut self.squad {
            let before = u.max_hp(&c);
            u.potions.clear();
            u.follow_max(&c, before);
        }
        let mut level_ups = Vec::new();
        for a in battle.player_xp() {
            let Some(i) = battle.fighters[a.fighter].squad_index else { continue };
            let gained = self.squad[i].gain_xp(&c, a.xp);
            if gained > 0 {
                level_ups.push((i, self.squad[i].level));
            }
        }
        // Then every unit the battle left dead loses its spell slots (0x4c50ec).
        for u in self.squad.iter_mut().filter(|u| !u.alive()) {
            u.spells = Default::default();
        }
        let now = self.clock.total_minutes() as u64;
        let mut dropped = Vec::new();
        let mut lost = 0;
        for u in self.squad.iter_mut().skip(1) {
            if u.hp <= 0 && u.died_at.is_none() {
                u.hp = 0;
                u.died_at = Some(now);
                u.unpaid = false;
                dropped.extend(u.items.iter_mut().filter_map(Option::take));
                lost += 1;
            }
        }
        let (_, mut dropped_left) = self.take_items(dropped);
        let foe = self.foe.take();
        // The opponent's record is recounted too (0x4d21fd), a garrison's as well.
        if let Some(Foe::Garrison(l)) = foe {
            self.world.locations[l].strengths_bare = false;
        }
        // The AI rescores its matchups with the hero and the army he fought (4c50ec); his
        // army is recounted (the victory report's layout 0x4a9b75, the window's close).
        self.mark_dirty(ai::HERO);
        self.recount_hero();
        if let Some(Foe::Army(i)) = foe {
            let uid = self.world.armies[i].uid;
            self.mark_dirty(uid);
        }
        let mana = if battle.outcome() == Outcome::Victory { self.surrender_mana(battle) } else { 0 };
        self.mana += mana;

        match (battle.outcome(), foe) {
            (Outcome::Victory, Some(Foe::Garrison(l))) => {
                let loc = &mut self.world.locations[l];
                loc.cleared = true;
                // Every item its units wore, in their order, then its pack (0x4c50ec, as for
                // an army): the ruins' goods its units put on at the load come back so.
                let worn: Vec<ItemId> = loc.garrison.iter_mut().flat_map(|t| t.worn.iter_mut().filter_map(Option::take)).collect();
                loc.garrison.clear();
                // The garrison's gold (ruins: their treasure), the building's stock and one
                // day's income; no division.
                let reward = std::mem::take(&mut loc.treasure_gold) + std::mem::take(&mut loc.tribute_gold) + loc.gold_income.max(0);
                let treasure = std::mem::take(&mut loc.treasure);
                let rolls = std::mem::take(&mut loc.loot_rolls);
                // Whatever it is (castle, fort, ruins…), a place whose garrison is beaten is
                // the hero's now; only the demo's bandit camps burn instead.
                let captured = (loc.kind != LocationKind::Camp).then_some(l);
                if captured.is_some() {
                    self.world.give_to_player(l);
                }
                self.gold += reward;
                let mut found = worn;
                found.extend(treasure);
                found.extend((0..rolls).filter_map(|_| self.roll_item(Source::Loot)));
                let (loot, left_behind) = self.take_items(found);
                dropped_left += left_behind;
                BattleResult::Victory { reward, mana, lost, loot, left_behind: dropped_left, level_ups, captured }
            }
            (Outcome::Victory, Some(Foe::Army(i))) => {
                let (gold, wages) = self.player_victory_gold(&self.world.armies[i]);
                let reward = gold + wages;
                // An army whose home castle or fort stands empty loses it to the player.
                let home = self.world.armies[i].home.filter(|&h| {
                    let l = &self.world.locations[h];
                    l.kind.capturable() && !l.owned() && l.garrison.is_empty()
                });
                let army = &mut self.world.armies[i];
                army.gold -= gold;
                let (id, mut found) = (army.id, Vec::new());
                // Every item its units wore, then its pack (economy.md §3).
                for t in army.troops.iter_mut() {
                    found.extend(t.worn.iter_mut().filter_map(Option::take));
                }
                found.append(&mut army.items);
                // Off the map; it may respawn (`rules::ai`).
                self.army_beaten(i, Beaten::ByPlayer);
                self.gold += reward;
                if id == 0 && self.rng.range(1, 100) <= GANG_LOOT_CHANCE {
                    found.extend(self.roll_item(Source::Loot));
                }
                let (loot, left_behind) = self.take_items(found);
                dropped_left += left_behind;
                if let Some(h) = home {
                    self.world.give_to_player(h);
                }
                BattleResult::Victory { reward, mana, lost, loot, left_behind: dropped_left, level_ups, captured: home }
            }
            (Outcome::Victory, None) => {
                BattleResult::Victory { reward: 0, mana, lost, loot: Vec::new(), left_behind: dropped_left, level_ups, captured: None }
            }
            (Outcome::Defeat, _) => BattleResult::Defeat,
            (_, foe) => {
                if let Some(Foe::Army(i)) = foe {
                    self.world.armies[i].ignore_until = self.clock.total_minutes() + 120.0;
                }
                BattleResult::Withdrew { lost }
            }
        }
    }

    pub fn won(&self) -> bool {
        self.world.all_camps_cleared()
    }

    /// A random item of the given source, if the table has any.
    pub(crate) fn roll_item(&mut self, source: Source) -> Option<ItemId> {
        let pool = self.content.items_from(source);
        if pool.is_empty() {
            return None;
        }
        Some(pool[self.rng.range(0, pool.len() as i32 - 1) as usize])
    }

    /// Puts found items into the pack. Returns (kept, left behind).
    pub(crate) fn take_items(&mut self, found: Vec<ItemId>) -> (Vec<ItemId>, usize) {
        let mut kept = Vec::new();
        let mut left_behind = 0;
        for item in found {
            if self.pack.len() < PACK_SIZE {
                self.pack.push(item);
                kept.push(item);
            } else {
                left_behind += 1;
            }
        }
        (kept, left_behind)
    }

    /// Items for sale where the party stands, if there is a market. Ill-disposed markets
    /// trade too, dearer ([`Game::buy_price`]; the footage shows a market of attitude −2
    /// trading).
    /// The goods of the market here, as its window lists them.
    pub fn market_here(&self) -> Option<Vec<ItemId>> {
        let loc = &self.world.locations[self.location?];
        loc.shop.as_ref().map(|s| s.goods())
    }

    /// Buys the `stock_index`-th good here for [`Game::buy_price`], if that is at most the
    /// gold (0x4b9e18). It goes to the pack with no test of its room, as in the original,
    /// and its place in the market is emptied.
    pub fn buy(&mut self, stock_index: usize) -> Result<ItemId, TradeError> {
        let item = *self.market_here().ok_or(TradeError::NoMarket)?.get(stock_index).ok_or(TradeError::NoSuchItem)?;
        let price = self.buy_price(item);
        if self.gold < price {
            return Err(TradeError::NotEnoughGold);
        }
        if let Some(shop) = self.location.and_then(|l| self.world.locations[l].shop.as_mut()) {
            shop.take(stock_index);
        }
        self.gold = (self.gold - price).max(0);
        self.pack.push(item);
        self.scan_on_close = true;
        Ok(item)
    }

    /// Sells a pack item for [`Game::sell_price`]. Returns the gold gained.
    pub fn sell(&mut self, pack_index: usize) -> Result<i32, TradeError> {
        self.market_here().ok_or(TradeError::NoMarket)?;
        let item = *self.pack.get(pack_index).ok_or(TradeError::NoSuchItem)?;
        if !self.can_sell(item) {
            return Err(TradeError::NotForSale);
        }
        let price = self.sell_price(item);
        self.pack.remove(pack_index);
        self.gold += price;
        self.scan_on_close = true;
        Ok(price)
    }

    /// Moves a pack item onto squad member `unit` into its lowest free slot, as a drop on its
    /// card in the army window does (wear rules in [`items::slot_for`]).
    pub fn equip(&mut self, unit: usize, pack_index: usize) -> Result<(), EquipError> {
        self.equip_at(unit, pack_index, None)
    }

    /// Moves a pack item onto squad member `unit`: into item slot `slot` when given, as a
    /// click on an empty worn slot of the hero window puts it in that very slot (0x4c24f4;
    /// an occupied slot takes nothing), else into the lowest free one. The wear test runs
    /// either way.
    pub fn equip_at(&mut self, unit: usize, pack_index: usize, slot: Option<usize>) -> Result<(), EquipError> {
        let item = *self.pack.get(pack_index).ok_or(EquipError::NoSuchItem)?;
        let u = self.squad.get(unit).ok_or(EquipError::NoSuchItem)?;
        let free = items::slot_for(&self.content, u, item)?;
        let slot = match slot {
            Some(k) if u.items.get(k) == Some(&None) => k,
            Some(_) => return Err(EquipError::NoFreeSlot),
            None => free,
        };
        items::put_on(&self.content, &mut self.squad[unit], slot, item);
        self.pack.remove(pack_index);
        Ok(())
    }

    /// Squad member `unit` drinks the potion at `pack_index` ([`items::drink`]). Returns the
    /// HP gained.
    pub fn drink(&mut self, unit: usize, pack_index: usize) -> Result<i32, EquipError> {
        let item = *self.pack.get(pack_index).ok_or(EquipError::NoSuchItem)?;
        let c = self.content.clone();
        let now = self.clock.total_minutes() as u64;
        let u = self.squad.get_mut(unit).ok_or(EquipError::NoSuchItem)?;
        let healed = items::drink(&c, u, item, now)?;
        self.pack.remove(pack_index);
        Ok(healed)
    }

    /// Moves item slot `slot` of squad member `unit` back into the pack (a dead unit's too);
    /// the unit is rebuilt.
    pub fn unequip(&mut self, unit: usize, slot: usize) -> Result<(), EquipError> {
        if self.pack.len() >= PACK_SIZE {
            return Err(EquipError::PackFull);
        }
        let c = self.content.clone();
        let u = self.squad.get_mut(unit).ok_or(EquipError::NoSuchItem)?;
        let item = items::take_off(&c, u, slot).ok_or(EquipError::NoSuchItem)?;
        self.pack.push(item);
        Ok(())
    }

    /// Hands the item in slot `slot` of squad member `from` straight to squad member `to`
    /// (dragged from one unit onto another on the army screen).
    pub fn give(&mut self, from: usize, slot: usize, to: usize) -> Result<(), EquipError> {
        let item = self.squad.get(from).and_then(|u| u.items.get(slot).copied().flatten()).ok_or(EquipError::NoSuchItem)?;
        let target = self.squad.get(to).ok_or(EquipError::NoSuchItem)?;
        let free = items::slot_for(&self.content, target, item)?;
        let c = self.content.clone();
        items::take_off(&c, &mut self.squad[from], slot);
        items::put_on(&c, &mut self.squad[to], free, item);
        Ok(())
    }
}


#[cfg(test)]
impl Game {
    /// Tests of the noon's payments: the start day's 12:00 counts (the original's first
    /// noon for the hero is the day after the start, world.md §6.4).
    pub(crate) fn first_noon_today(&mut self) {
        self.start_day = self.clock.day_index().saturating_sub(1);
    }

    /// Walks towards `target` as a player would through the fog (tests on real maps): the
    /// target itself once it can be clicked, else the explored cell nearest it that has a
    /// route, leg after leg, until he is there, stopped by an army or a garrison, or stuck.
    pub(crate) fn walk_through_fog(&mut self, target: Tile) -> Vec<Event> {
        let mut events = Vec::new();
        let goal_building = self.world.location_at(target);
        for _ in 0..400 {
            let here = self.tile();
            if self.foe.is_some() || here == target || (goal_building.is_some() && self.location == goal_building) {
                break;
            }
            let leg = if !self.plan(target).is_empty() {
                Some(target)
            } else {
                // The farthest explored cell he can plan to along the way he would take with
                // the whole map in sight.
                let map = &self.world.map;
                let ideal = map.flood_route(&|t| self.planner_cost(t), &|_| 1, &[(target, 0)], here).map(|r| r.0).unwrap_or_default();
                ideal.into_iter().rev().filter(|&t| self.fog.explored(t)).take(40).find(|&t| t != (0, 0) && !self.plan(t).is_empty())
            };
            let Some(leg) = leg else { break };
            if !self.set_destination(leg) {
                break;
            }
            for _ in 0..20_000 {
                if !self.moving() {
                    break;
                }
                events.extend(self.tick(0.05));
            }
        }
        events
    }
}

/// The event engine's archetype code of a hero class.
pub(crate) fn archetype_of(hero: HeroClass) -> u8 {
    match hero {
        HeroClass::Knight => 1,
        HeroClass::Archmage => 2,
        HeroClass::Ranger => 3,
    }
}

/// The cells the original's auto-arrange (483b3c) gives `side`, by their indices: the side
/// put through a battle and back (0x49855c, 0x4988c0).
fn arranged(content: &Arc<Content>, side: &[(usize, &Unit)], defence: i32) -> Vec<(usize, Slot)> {
    if side.is_empty() {
        return Vec::new();
    }
    let mut b = Battle::new(content.clone(), side, &[], Team::Player);
    b.set_building_defence(Team::Player, defence);
    b.auto_arrange(Team::Player);
    b.fighters.iter().filter_map(|f| Some((f.squad_index?, f.slot))).collect()
}

/// An army's or a garrison's troops put through a battle side and back (0x49855c, 0x4988c0)
/// in a building of defence `defence`: the living take the cells the auto-arrange gives them.
pub(crate) fn arrange_troops(content: &Arc<Content>, troops: &mut [Troop], defence: i32) {
    let units: Vec<Unit> = troops.iter().map(|t| troop_unit(content, t)).collect();
    let side: Vec<(usize, &Unit)> = units.iter().enumerate().filter(|(k, _)| troops[*k].alive()).collect();
    for (k, s) in arranged(content, &side, defence) {
        troops[k].slot = s;
    }
}

/// The unit of an army or garrison troop: its level and XP, its worn items, its pay and
/// kind, its hit points (its maximum, items included, minus what it lacks; 0 dead).
pub(crate) fn troop_unit(content: &Content, t: &Troop) -> Unit {
    troop_unit_stats(content, t).0
}

/// [`troop_unit`] with its current stats ([`Unit::stats`]), rebuilt once for both: the stats
/// do not depend on the HP.
pub(crate) fn troop_unit_stats(content: &Content, t: &Troop) -> (Unit, super::units::Stats) {
    let mut u = Unit::new(content, t.unit, t.slot);
    u.level = t.level.max(1);
    u.xp = t.xp;
    u.items = t.worn;
    u.wage_kind = t.kind;
    u.unpaid = t.unpaid;
    u.last_paid = t.last_paid;
    u.spells = t.spells;
    u.drain = t.drain;
    u.carry = t.carry;
    u.named = t.named;
    u.personal = t.personal;
    // Healed full (`Unit::heal_full`), then its wounds.
    let stats = u.stats(content);
    u.hp = stats.max_hp();
    if t.alive() {
        u.hp = (u.hp - t.hurt).max(1);
    } else {
        u.hp = 0;
        u.died_at = t.died_at;
    }
    (u, stats)
}

/// The whole record of unit `u` as an army's troop (the reverse of [`troop_unit`]): level,
/// XP, worn items, pay and kind, spells, wounds or death, name and personal items.
pub(crate) fn troop_of_unit(content: &Content, u: &Unit, now: u64) -> Troop {
    let mut t = Troop::new(u.def, u.level, u.slot);
    t.xp = u.xp;
    t.worn = u.items;
    t.unpaid = u.unpaid;
    t.last_paid = u.last_paid;
    t.kind = u.wage_kind;
    t.named = u.named;
    t.personal = u.personal;
    t.died_at = u.died_at;
    unit_into_troop(content, &mut t, u, now);
    t
}

/// Writes what a world spell or a rebuild did to the unit of troop `t` back into it: its
/// slots, drain and HP carry, and its HP as what it lacks, or its death (the time of death
/// now, unless it keeps one from an earlier death, as `ai::write_hp`); a unit raised again
/// keeps its time of death.
pub(crate) fn unit_into_troop(content: &Content, t: &mut Troop, u: &Unit, now: u64) {
    t.spells = u.spells;
    t.drain = u.drain;
    t.carry = u.carry;
    if u.alive() {
        if t.died_at.is_some() {
            t.kept_death = t.died_at.take();
        }
        t.hurt = (u.max_hp(content) - u.hp).max(0);
    } else if t.died_at.is_none() {
        t.died_at = Some(t.kept_death.take().unwrap_or(now));
    }
}

/// An event of the scenario fired (or asks its question) in this event.
pub(crate) fn fires(e: &Event) -> bool {
    matches!(e, Event::Script(super::events::EventOutcome::Fired { .. } | super::events::EventOutcome::Question(_)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::world::{demo_unit, GANG_REWARD};

    fn distance(a: (f32, f32), b: (f32, f32)) -> f32 {
        ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
    }

    fn content() -> Arc<Content> {
        Arc::new(Content::builtin())
    }

    /// A new demo game whose generator then stands at `seed` (a new map always starts it
    /// at 1; tests vary it).
    fn new_game(hero: HeroClass, seed: u32) -> Game {
        let mut g = Game::new(content(), hero);
        g.rng = Rng::new(seed);
        g
    }

    #[test]
    fn the_front_row_width_switches_in_a_running_game() {
        use crate::rules::formation::{Formation, Row};
        let mut g = new_game(HeroClass::Knight, 1);
        assert_eq!(g.content.formation, Formation::WIDE);
        // Put the hero on the front row's edge cell, which the 4-wide shape lacks.
        g.squad[0].slot = Slot::new(Row::Front, 5);
        let kept: Vec<Slot> = g.squad[1..].iter().map(|u| u.slot).collect();
        g.set_formation(Formation::VANILLA);
        assert_eq!(g.content.formation, Formation::VANILLA);
        assert!(!g.wide_row, "a save records the new width");
        assert!(g.squad.iter().all(|u| Formation::VANILLA.contains(u.slot)), "every unit on a cell of the new shape");
        let mut slots: Vec<Slot> = g.squad.iter().map(|u| u.slot).collect();
        slots.sort_by_key(|s| (s.row, s.col));
        slots.dedup();
        assert_eq!(slots.len(), g.squad.len(), "no two units on one cell");
        for (u, s) in g.squad[1..].iter().zip(&kept) {
            if Formation::VANILLA.contains(*s) {
                assert_eq!(u.slot, *s, "a unit whose cell the new shape has keeps it");
            }
        }
        // Back to 6: nobody moves, the width is recorded again.
        let before: Vec<Slot> = g.squad.iter().map(|u| u.slot).collect();
        g.set_formation(Formation::WIDE);
        assert!(g.wide_row);
        assert_eq!(g.squad.iter().map(|u| u.slot).collect::<Vec<_>>(), before);
    }

    /// A game with no gangs on the map, for tests about travel and time.
    fn quiet_game(hero: HeroClass) -> Game {
        let mut g = new_game(hero, 1);
        g.world.armies.clear();
        g
    }

    fn unit(g: &Game, key: &str) -> UnitId {
        demo_unit(&g.content, key)
    }

    fn item(g: &Game, key: &str) -> ItemId {
        g.content.item_by_key(key).unwrap()
    }

    fn walk_until_stopped(g: &mut Game) -> Vec<Event> {
        let mut events = Vec::new();
        for _ in 0..10_000 {
            if !g.moving() {
                break;
            }
            events.extend(g.tick(0.05));
        }
        events
    }

    fn tile_of_location(g: &Game, name: &str) -> Tile {
        g.world.locations[g.world.index_of(name)].tile
    }

    /// Everyone but the player's hero drops dead.
    fn wipe_all_but_hero(b: &mut Battle) {
        for f in b.fighters.iter_mut().filter(|f| !f.is_hero) {
            f.hp = 0;
        }
    }

    #[test]
    fn starts_at_home_castle_in_the_morning() {
        let g = new_game(HeroClass::Knight, 1);
        assert_eq!(g.location, Some(0));
        assert_eq!(g.clock, Clock::demo_start());
        assert_eq!((g.hero().def, g.gold), (HeroClass::Knight.unit(), 100));
        assert_eq!(g.max_squad(), 12);
    }

    #[test]
    fn time_is_frozen_while_standing_still() {
        let mut g = new_game(HeroClass::Knight, 1);
        let parties: Vec<_> = g.world.armies.iter().map(|p| p.pos).collect();
        assert!(g.tick(5.0).is_empty());
        assert_eq!(g.clock, Clock::demo_start());
        assert_eq!(parties, g.world.armies.iter().map(|p| p.pos).collect::<Vec<_>>());
    }

    #[test]
    fn walking_to_a_village_takes_time_and_arrives() {
        let mut g = quiet_game(HeroClass::Knight);
        let millbrook = g.world.index_of("Millbrook");
        assert!(g.set_destination(tile_of_location(&g, "Millbrook")));
        assert_eq!(g.location, Some(0), "in Oakford until he steps off");
        let events = walk_until_stopped(&mut g);
        // The village makes no offer on this stream, so its tribute is taken on arrival.
        let n = events.len();
        assert_eq!(events[n - 2], Event::Arrived(millbrook));
        assert!(matches!(events[n - 1], Event::Tribute { at, .. } if at == millbrook), "{events:?}");
        assert_eq!(g.location, Some(millbrook));
        assert!(g.clock.total_minutes() > Clock::demo_start().total_minutes() + 60.0);
    }

    #[test]
    fn a_buildings_window_marks_the_heros_ai_pairs_dirty() {
        // 0x4ba854 recomputes his army with 0x497240(0, 1): the AI rescores him at its next
        // plan.
        let mut g = new_game(HeroClass::Knight, 1);
        g.world.armies.truncate(1);
        let a = &mut g.world.armies[0];
        (a.attitude, a.patrols, a.patrol_radius) = (3, true, 0);
        a.pos = g.world.map.center((0, 0));
        a.mind.clean.insert(ai::HERO);
        assert!(g.set_destination(tile_of_location(&g, "Millbrook")));
        walk_until_stopped(&mut g);
        assert_eq!(g.location, Some(g.world.index_of("Millbrook")));
        assert!(!g.world.armies[0].mind.clean.contains(&ai::HERO));
    }

    #[test]
    fn cannot_walk_into_the_sea() {
        let mut g = quiet_game(HeroClass::Knight);
        assert!(!g.set_destination((30, 40)), "open sea");
    }

    #[test]
    fn noon_pays_income_and_wages_and_marks_unpaid() {
        let mut g = quiet_game(HeroClass::Knight);
        g.first_noon_today();
        g.hire(unit(&g, "spearman")).unwrap();
        g.hire(unit(&g, "archer")).unwrap();
        g.gold = 0;
        g.squad[1].hp = 1;
        // Oakford pays its stock at noon; it grows only at midnight, so give it a day's.
        let oak = g.world.index_of("Oakford");
        g.world.locations[oak].tribute_gold = 20;
        let day = g.clock.day_index();
        let mut events = Vec::new();
        g.pass_time(3.0 * 60.0, &mut events); // 08:00 -> 11:00
        assert!(events.is_empty());
        g.pass_time(60.0, &mut events); // 12:00
        // Income 20; wages from cost: spearman 40/2×¼ = 5, archer 45/2×¼ = 5.6 → 6.
        let report = |day, wages, unpaid, gold| DayReport {
            day,
            income: 20,
            wages,
            mana_wages: 0,
            unpaid,
            deserted: vec![],
            gold,
            mana_total: 0,
        };
        // The report shows the gold before the payment.
        assert_eq!(events, vec![Event::NewDay(report(day, 11, 0, 0))]);
        assert_eq!(g.gold, 9);

        g.gold = -20; // broke: still short after the stock (20 again since midnight)
        events.clear();
        g.pass_time(24.0 * 60.0, &mut events);
        assert_eq!(events, vec![Event::NewDay(report(day + 1, 11, 2, -20))]);
        assert_eq!(g.gold, 0);
        assert!(g.squad[1].unpaid && g.squad[2].unpaid);
    }

    #[test]
    fn the_first_noon_is_the_day_after_the_start_and_the_clock_starts_a_minute_late() {
        // World.md §6.3–6.4: the clock reads the DTm start minute + 1; the next noon at the
        // start is (start div 1440 + 1) × 1440 + 720, so a 09:00 start skips that day's noon.
        let mut g = start(&strip());
        let t0 = g.clock.total_minutes();
        assert_eq!(t0, 624_354_300.0 + 1.0);
        let mut events = Vec::new();
        g.pass_time(4.0 * 60.0, &mut events);
        assert!(events.is_empty(), "13:01 on the start day: no noon report");
        g.pass_time(24.0 * 60.0, &mut events);
        assert!(matches!(events.as_slice(), [Event::NewDay(r)] if r.day == g.start_day + 1), "{events:?}");
    }

    #[test]
    fn a_noon_paid_after_midnight_skips_that_days_noon() {
        // 0x4a41d8 sets the next noon to the day after the moment the noon is paid: held up
        // past 00:00 by a reading, the start day's noon comes at 01:30 and the next day's
        // 12:00 brings none; the one after does.
        let mut g = start(&strip());
        g.first_noon_today();
        g.mana = 1000;
        g.spells = vec![1];
        let d = g.clock.day_index();
        g.reading = Some(magic::Reading { spell: 1, target: magic::CastTarget::Own });
        let mut events = Vec::new();
        g.pass_time(16.0 * 60.0, &mut events);
        assert!(events.is_empty() && g.noon_due == Some(d), "{events:?}");
        g.reading = None;
        g.pass_time(30.0, &mut events);
        assert!(matches!(events.as_slice(), [Event::NewDay(r)] if r.day == d), "{events:?}");
        events.clear();
        g.pass_time(24.0 * 60.0, &mut events);
        assert!(events.is_empty(), "day {}'s noon is skipped: {events:?}", d + 1);
        g.pass_time(24.0 * 60.0, &mut events);
        assert!(matches!(events.as_slice(), [Event::NewDay(r)] if r.day == d + 2), "{events:?}");
    }

    #[test]
    fn the_noon_waits_for_a_scan_with_no_event_and_no_spell_being_read() {
        // World.md §6.4 (0x4abfbc): checked in the event scan when nothing fired and no
        // spell is being cast; 00:00 comes after the armies moved.
        let mut g = start(&strip());
        g.first_noon_today();
        g.mana = 1000;
        g.spells = vec![1];
        // A reading across 12:00: the report comes after it, at the next scan.
        let t0 = g.clock.total_minutes();
        g.reading = Some(magic::Reading { spell: 1, target: magic::CastTarget::Own });
        let mut events = Vec::new();
        g.pass_time(4.0 * 60.0, &mut events);
        assert!(g.clock.total_minutes() > t0 + 3.0 * 60.0 && events.is_empty(), "{events:?}");
        assert!(g.noon_due.is_some());
        g.reading = None;
        g.pass_time(30.0, &mut events);
        assert!(matches!(events.as_slice(), [Event::NewDay(_)]), "{events:?}");
        assert!(g.noon_due.is_none());
    }

    #[test]
    fn villages_refill_at_midnight() {
        let mut g = quiet_game(HeroClass::Knight);
        g.location = Some(g.world.index_of("Millbrook"));
        assert_eq!(g.tribute_available(), Some(10));
        assert!(g.collect_tribute().is_some());
        assert_eq!(g.tribute_available(), None);
        let mut events = Vec::new();
        g.pass_time(15.0 * 60.0, &mut events); // 08:00 -> 23:00: no noon on the start day
        assert!(events.is_empty());
        assert_eq!(g.tribute_available(), None);
        g.pass_time(60.0, &mut events); // 00:00
        assert_eq!(g.tribute_available(), Some(10));
    }

    #[test]
    fn waiting_passes_time_and_moves_the_world() {
        let mut g = new_game(HeroClass::Knight, 3);
        g.first_noon_today();
        let start = g.clock.total_minutes();
        let before: Vec<_> = g.world.armies.iter().map(|p| p.pos).collect();
        let events = g.wait(4);
        assert_eq!(g.clock.total_minutes(), start + 240.0);
        assert!(events.iter().any(|e| matches!(e, Event::NewDay(_))), "08:00 + 4 h crosses noon");
        assert_ne!(before, g.world.armies.iter().map(|p| p.pos).collect::<Vec<_>>(), "gangs patrol meanwhile");
        g.wait(1);
        assert_eq!(g.clock.total_minutes(), start + 300.0);
        assert!(!g.moving());
    }

    #[test]
    fn ranger_heals_the_army_every_day() {
        let mut g = quiet_game(HeroClass::Ranger);
        g.first_noon_today();
        g.squad[0].hp = 10;
        let mut events = Vec::new();
        g.pass_time(4.0 * 60.0, &mut events); // noon
        assert_eq!(g.hero().hp, 10 + 55 * 15 / 100 + 55 * 20 / 100, "15% at noon, 20% more as the report is shown");
        // No wages and no income: no report, so only the 15%.
        let mut r = quiet_game(HeroClass::Ranger);
        r.first_noon_today();
        for l in r.world.locations.iter_mut().filter(|l| l.owned()) {
            l.owner = crate::rules::world::Owner::Neutral;
        }
        r.squad[0].hp = 10;
        let mut quiet = Vec::new();
        r.pass_time(4.0 * 60.0, &mut quiet);
        assert!(quiet.iter().all(|e| !matches!(e, Event::NewDay(_))), "{quiet:?}");
        assert_eq!(r.hero().hp, 10 + 55 * 15 / 100);
        let mut k = quiet_game(HeroClass::Knight);
        k.squad[0].hp = 10;
        k.pass_time(4.0 * 60.0, &mut events);
        assert_eq!(k.hero().hp, 10, "only the Ranger");
    }

    #[test]
    fn unpaid_units_sit_out_battles() {
        let mut g = quiet_game(HeroClass::Knight);
        let spear = unit(&g, "spearman");
        g.hire(spear).unwrap();
        g.squad[1].unpaid = true;
        g.foe = Some(Foe::Garrison(g.world.index_of("Bandit camp")));
        let b = g.start_battle();
        assert!(b.fighters.iter().all(|f| f.unit != spear));
        assert_eq!(b.attacker, Team::Player, "walking into a camp is an attack");
        // Attacked, the player defends with all his living units, the unpaid too (49855c).
        let mut g = new_game(HeroClass::Knight, 1);
        g.hire(spear).unwrap();
        g.squad[1].unpaid = true;
        g.foe = Some(Foe::Army(0));
        let b = g.start_battle();
        assert_eq!(b.attacker, Team::Enemy);
        assert!(b.fighters.iter().any(|f| f.unit == spear));
    }

    #[test]
    fn after_a_battle_the_formation_is_the_battle_grid() {
        let mut g = quiet_game(HeroClass::Knight);
        let spear = unit(&g, "spearman");
        g.hire(spear).unwrap();
        g.hire(spear).unwrap();
        g.squad[2].unpaid = true;
        g.foe = Some(Foe::Garrison(g.world.index_of("Bandit camp")));
        let mut b = g.start_battle();
        b.begin();
        // The hero falls, the first spearman ends the battle on a cell of his own choosing.
        let hero = b.fighters.iter().position(|f| f.squad_index == Some(0)).unwrap();
        let first = b.fighters.iter().position(|f| f.squad_index == Some(1)).unwrap();
        b.fighters[hero].hp = 0;
        b.fighters[first].slot = Slot::new(crate::rules::formation::Row::Front, 0);
        wipe_enemies(&mut b);
        g.settle_battle(&b);
        assert_eq!(g.squad[1].slot, Slot::new(crate::rules::formation::Row::Front, 0), "the cell it ended on");
        // Those without a cell take free ones, reserve first: the spearman who sat out, then
        // the fallen hero (back at 1 HP).
        let f = g.content.formation;
        let first_free = f.after_battle_slot(&[Slot::new(crate::rules::formation::Row::Front, 0)]).unwrap();
        // Columns in plain order: the first open reserve cell, not the preferred column.
        let reserve = crate::rules::formation::Row::Reserve;
        assert_eq!(first_free, if f.cols == 6 { Slot::new(reserve, 2) } else { Slot::new(reserve, 0) });
        assert_eq!(g.squad[2].slot, first_free);
        assert_eq!(g.squad[0].slot, f.after_battle_slot(&[Slot::new(crate::rules::formation::Row::Front, 0), first_free]).unwrap());
        assert_eq!(g.squad[0].hp, 1);
    }

    #[test]
    fn village_serves_once_per_day() {
        let mut g = quiet_game(HeroClass::Knight);
        g.set_destination(tile_of_location(&g, "Millbrook"));
        let events = walk_until_stopped(&mut g);
        // No offer on this stream: the tribute (an item) is taken on arrival.
        assert!(events.iter().any(|e| matches!(e, Event::Tribute { paid: Tribute::Item(_), .. })), "{events:?}");
        assert_eq!(g.collect_tribute(), None, "already collected");
        let mut events = Vec::new();
        g.pass_time(24.0 * 60.0, &mut events);
        assert_eq!(g.tribute_available(), Some(10));
    }

    #[test]
    fn castle_healing_is_paid() {
        let mut g = quiet_game(HeroClass::Knight);
        g.first_noon_today();
        g.gold = 1000;
        g.squad[0].hp = 5;
        g.set_destination(tile_of_location(&g, "Millbrook"));
        walk_until_stopped(&mut g);
        g.set_destination(tile_of_location(&g, "Oakford"));
        walk_until_stopped(&mut g);
        assert_eq!(g.hero().hp, 5, "no free healing on arrival");
        let price = g.heal_price(0).unwrap();
        let gold = g.gold;
        g.heal(0).unwrap();
        assert_eq!((g.hero().hp, g.gold), (70, gold - price.amount));
    }

    #[test]
    fn walking_into_a_gang_starts_an_encounter() {
        let mut g = quiet_game(HeroClass::Knight);
        let target = tile_of_location(&g, "Millbrook");
        g.set_destination(target);
        // Two tiles ahead: inside the chase radius, so it closes in.
        let ahead = g.path[1];
        let camp = g.world.index_of("Bandit camp");
        g.world.spawn_gang(camp, ahead);
        let events = walk_until_stopped(&mut g);
        assert!(matches!(events.last(), Some(Event::Encounter(0))), "{events:?}");
        assert_eq!(g.foe, Some(Foe::Army(0)));
        assert!(!g.moving());
        assert_eq!(g.start_battle().attacker, Team::Enemy, "the gang attacks");
    }

    #[test]
    fn gangs_chase_a_nearby_party() {
        let mut g = quiet_game(HeroClass::Knight);
        g.set_destination(tile_of_location(&g, "Millbrook"));
        let camp = g.world.index_of("Bandit camp");
        let start = (g.tile().0 + 3, g.tile().1 + 2);
        assert!(g.world.map.passable(start));
        g.world.spawn_gang(camp, start);
        let before = distance(g.world.armies[0].pos, g.pos);
        for _ in 0..3 {
            g.tick(0.05);
        }
        assert!(g.world.armies[0].chasing);
        assert!(distance(g.world.armies[0].pos, g.pos) < before + 0.5, "it keeps up");
    }

    #[test]
    fn beating_a_gang_removes_it_pays_and_gives_xp() {
        let mut g = quiet_game(HeroClass::Knight);
        let camp = g.world.index_of("Bandit camp");
        g.world.spawn_gang(camp, (30, 20));
        g.foe = Some(Foe::Army(0));
        let mut b = g.start_battle();
        b.begin();
        wipe_all_but_hero(&mut b);
        let gold = g.gold;
        // Half its gold, and its daily wages.
        let want = GANG_REWARD + crate::rules::ai::army_wages(&g.content, &g.world.armies[0].troops);
        assert!(matches!(g.resolve_battle(&b), BattleResult::Victory { reward, lost: 0, captured: None, .. } if reward == want));
        assert_eq!(g.gold, gold + want);
        assert!(g.world.armies.is_empty());
        assert_eq!(g.foe, None);
        assert!(g.hero().xp > 0 || g.hero().level > 1, "XP after the battle");
    }

    #[test]
    fn reaching_the_turn_limit_against_a_gang_is_a_victory() {
        // The original has no draw: after the first action of turn `BattleEndTurn` the
        // player wins if any of his units stand (battle.md §5).
        let mut g = quiet_game(HeroClass::Knight);
        let camp = g.world.index_of("Bandit camp");
        g.world.spawn_gang(camp, (30, 20));
        g.foe = Some(Foe::Army(0));
        let mut b = g.start_battle();
        b.begin();
        while b.outcome() == Outcome::Ongoing {
            b.skip();
        }
        assert_eq!(b.round, 25);
        // The loot itself follows economy.md (half the gang's gold plus its wages).
        assert!(matches!(g.resolve_battle(&b), BattleResult::Victory { .. }));
        assert!(g.world.armies.is_empty(), "the gang counts as beaten");
    }

    #[test]
    fn the_beaten_armys_correction_scales_the_players_xp() {
        let gain = |correction: i32| {
            let mut g = quiet_game(HeroClass::Knight);
            let camp = g.world.index_of("Bandit camp");
            g.world.spawn_gang(camp, (30, 20));
            g.world.armies[0].ai.exp_correction = correction;
            g.foe = Some(Foe::Army(0));
            let mut b = g.start_battle();
            b.begin();
            wipe_all_but_hero(&mut b);
            let share = b.xp_awards(Team::Player)[0].xp;
            let xp = b.player_xp()[0].xp;
            g.resolve_battle(&b);
            assert_eq!(g.hero().xp > 0 || g.hero().level > 1, xp > 0);
            (share, xp)
        };
        let (share, normal) = gain(100);
        let (_, double) = gain(200);
        // The demo's options: modifier 100, difficulty 100.
        assert_eq!(normal, share);
        assert_eq!(double, 2 * share);
        // A correction of 0 is used as it is: no XP (it was read as 100 before).
        assert_eq!(gain(0).1, 0);
    }

    #[test]
    fn the_hero_is_not_promoted_and_promotion_is_free() {
        let mut g = quiet_game(HeroClass::Knight);
        let (spear, sword) = (unit(&g, "spearman"), unit(&g, "swordsman"));
        g.hire(spear).unwrap();
        g.squad[1].level = 3;
        g.squad[1].xp = 40;
        g.squad[0].level = 5;
        assert_eq!(g.promote(0, sword), Err(PromoteError::NotAvailable), "the hero rises by levels only");
        let gold = g.gold;
        g.promote(1, sword).unwrap();
        assert_eq!((g.squad[1].def, g.squad[1].level, g.squad[1].xp, g.gold), (sword, 1, 0, gold));
    }

    #[test]
    fn camps_send_out_new_gangs_every_few_days() {
        let mut g = quiet_game(HeroClass::Knight);
        let mut events = Vec::new();
        // 08:00: the start day's noon is not the hero's; then two noons.
        g.pass_time((4 + 48) as f32 * 60.0, &mut events);
        assert!(g.world.armies.is_empty());
        g.pass_time(24.0 * 60.0, &mut events); // the third noon
        assert_eq!(g.world.armies.len(), 2, "one gang from each camp");
    }

    #[test]
    fn hire_checks_offer_gold_and_cap() {
        let mut g = quiet_game(HeroClass::Knight);
        let (spear, archer, sword) = (unit(&g, "spearman"), unit(&g, "archer"), unit(&g, "swordsman"));
        g.hire(spear).unwrap();
        assert_eq!(g.gold, 60);
        // A new unit takes the first free cell from the reserve forward, whatever it is (495ce0).
        assert_eq!(g.squad[1].slot, g.content.formation.new_unit_slot(&[g.squad[0].slot]).unwrap());
        assert_eq!(g.squad[1].slot.row, crate::rules::formation::Row::Reserve);
        assert_eq!(g.hire(sword), Err(HireError::NotOffered));
        g.gold = 10_000;
        while g.squad.len() < g.max_squad() {
            g.hire(archer).unwrap();
        }
        assert_eq!(g.hire(archer), Err(HireError::SquadFull));
        g.location = None;
        assert_eq!(g.hire(archer), Err(HireError::NotOffered));
    }

    #[test]
    fn camp_victory_clears_and_pays() {
        let mut g = quiet_game(HeroClass::Knight);
        g.hire(unit(&g, "spearman")).unwrap();
        let camp = g.world.index_of("Bandit camp");
        g.foe = Some(Foe::Garrison(camp));
        let mut b = g.start_battle();
        b.begin();
        wipe_all_but_hero(&mut b);
        let result = g.resolve_battle(&b);
        assert!(matches!(&result, BattleResult::Victory { reward: 100, lost: 1, loot, left_behind: 0, captured: None, .. } if loot.len() == 1));
        assert_eq!(g.pack.len(), 1);
        assert!(g.world.locations[camp].cleared);
        assert!(!g.won());
    }

    #[test]
    fn a_fallen_hero_survives_if_his_army_does() {
        let mut g = quiet_game(HeroClass::Knight);
        g.hire(unit(&g, "spearman")).unwrap();
        g.foe = Some(Foe::Garrison(g.world.index_of("Bandit camp")));
        let mut b = g.start_battle();
        b.begin();
        for f in b.fighters.iter_mut().filter(|f| f.is_hero || f.team == Team::Enemy) {
            f.hp = 0;
        }
        assert!(matches!(g.resolve_battle(&b), BattleResult::Victory { lost: 0, .. }));
        assert_eq!(g.hero().hp, 1);
    }

    #[test]
    fn losing_the_whole_army_is_defeat() {
        let mut g = quiet_game(HeroClass::Knight);
        g.foe = Some(Foe::Garrison(g.world.index_of("Bandit camp")));
        let mut b = g.start_battle();
        b.begin();
        b.fighters[0].hp = 0;
        assert_eq!(g.resolve_battle(&b), BattleResult::Defeat);
    }

    #[test]
    fn auto_played_camp_battles_always_finish() {
        for seed in 0..10 {
            for hero in HeroClass::ALL {
                let mut g = new_game(hero, seed);
                g.world.armies.clear();
                g.hire(unit(&g, "spearman")).unwrap();
                g.foe = Some(Foe::Garrison(g.world.index_of(if seed % 2 == 0 { "Bandit camp" } else { "Bandit lair" })));
                let mut b = g.start_battle();
                b.begin();
                let mut steps = 0;
                while b.outcome() == Outcome::Ongoing {
                    b.ai_step();
                    steps += 1;
                    assert!(steps < 5000, "seed {seed}: battle never ended");
                }
                g.resolve_battle(&b);
            }
        }
    }

    #[test]
    fn a_quick_battle_resolves_exactly_like_a_played_one() {
        let setup = |seed: u32, lair: bool| {
            let mut g = new_game(HeroClass::Knight, seed);
            g.world.armies.clear();
            g.hire(unit(&g, "spearman")).unwrap();
            g.hire(unit(&g, "spearman")).unwrap();
            g.foe = Some(Foe::Garrison(g.world.index_of(if lair { "Bandit lair" } else { "Bandit camp" })));
            g
        };
        let json = |g: &Game| serde_json::to_string(g).unwrap();
        let mut outcomes = Vec::new();
        for seed in 0..6 {
            let (mut quick, mut played) = (setup(seed, seed % 2 == 1), setup(seed, seed % 2 == 1));
            let mut b = quick.start_battle();
            let outcome = b.auto_play_to_end();
            assert_ne!(outcome, Outcome::Ongoing);
            let quick_result = quick.resolve_battle(&b);
            let mut b = played.start_battle();
            b.begin();
            while b.outcome() == Outcome::Ongoing {
                b.ai_step();
            }
            let played_result = played.resolve_battle(&b);
            assert_eq!(quick_result, played_result, "seed {seed}");
            assert_eq!(json(&quick), json(&played), "seed {seed}: the same game after it");
            assert_eq!(quick.drain_events(), played.drain_events());
            outcomes.push(outcome);
        }
        assert!(outcomes.contains(&Outcome::Victory));
    }

    #[test]
    fn level_ups_are_reported() {
        let mut g = quiet_game(HeroClass::Knight);
        g.foe = Some(Foe::Garrison(g.world.index_of("Bandit lair")));
        g.squad[0].xp = g.squad[0].xp_to_next(&g.content) - 1;
        let mut b = g.start_battle();
        b.begin();
        wipe_all_but_hero(&mut b);
        let BattleResult::Victory { level_ups, .. } = g.resolve_battle(&b) else { panic!() };
        assert!(g.hero().level >= 2);
        assert_eq!(level_ups, vec![(0, g.hero().level)]);
    }

    #[test]
    fn promotion_needs_a_level_and_leaves_the_pack_alone() {
        let mut g = quiet_game(HeroClass::Knight);
        let (spear, sword) = (unit(&g, "spearman"), unit(&g, "swordsman"));
        g.hire(spear).unwrap();
        assert_eq!(g.promote(1, sword), Err(PromoteError::NotAvailable));
        g.squad[1].level = 2;
        let pack = g.pack.clone();
        g.promote(1, sword).unwrap();
        assert_eq!((g.squad[1].def, g.squad[1].level, &g.pack), (sword, 1, &pack));
    }

    fn at_oakford(g: &mut Game) {
        g.location = Some(g.world.index_of("Oakford"));
    }

    #[test]
    fn markets_stock_market_items_and_restock_every_midnight() {
        let mut g = quiet_game(HeroClass::Knight);
        at_oakford(&mut g);
        let stock = g.market_here().unwrap().to_vec();
        assert_eq!(stock.len(), MARKET_STOCK);
        assert!(stock.iter().all(|&i| g.content.sources(i).contains(&Source::Market)));
        g.gold = 10_000;
        g.buy(0).unwrap();
        assert_eq!(g.market_here().unwrap().len(), MARKET_STOCK - 1);
        let mut events = Vec::new();
        g.pass_time(15.0 * 60.0, &mut events); // 08:00 -> 23:00
        assert_eq!(g.market_here().unwrap().len(), MARKET_STOCK - 1, "not midnight yet");
        g.pass_time(60.0, &mut events);
        assert_eq!(g.market_here().unwrap().len(), MARKET_STOCK, "drawn anew at midnight");
    }

    #[test]
    fn buying_and_selling() {
        let mut g = quiet_game(HeroClass::Knight);
        g.location = None;
        assert_eq!(g.buy(0), Err(TradeError::NoMarket), "on the road");
        at_oakford(&mut g);
        let item = g.market_here().unwrap()[0];
        let cost = g.content.item(item).cost;
        let price = g.buy_price(item);
        assert_eq!(price, crate::rules::economy::relation_price(cost, 3, true), "his own castle: ×0.75");
        g.gold = price - 1;
        assert_eq!(g.buy(0), Err(TradeError::NotEnoughGold));
        g.gold = price;
        assert_eq!(g.buy(0), Ok(item));
        assert_eq!((g.gold, g.pack.clone()), (0, vec![item]));
        assert_eq!(g.sell(0), Ok(cost / 4), "ItemSaleCost 25% (the demo's F is 100)");
        assert!(g.pack.is_empty());
        assert_eq!(g.sell(0), Err(TradeError::NoSuchItem));
        // No pack test at a purchase (0x4b9e18).
        g.pack = vec![item; PACK_SIZE];
        g.gold = 10_000;
        assert!(g.buy(0).is_ok());
        assert_eq!(g.pack.len(), PACK_SIZE + 1);
        g.location = Some(g.world.index_of("Millbrook"));
        assert_eq!(g.sell(0), Err(TradeError::NoMarket));
    }

    #[test]
    fn equip_and_unequip_through_the_pack() {
        let mut g = quiet_game(HeroClass::Knight);
        let (sword, axe, shield, bow) = (item(&g, "short_sword"), item(&g, "war_axe"), item(&g, "oak_shield"), item(&g, "hunting_bow"));
        g.pack = vec![sword, axe, shield, bow];
        g.equip(0, 0).unwrap();
        assert_eq!(g.equip(0, 0), Err(EquipError::SecondWeapon), "axe is a second weapon");
        assert_eq!(g.equip(0, 2), Err(EquipError::WrongClass), "the knight is no archer");
        g.equip(0, 1).unwrap();
        assert_eq!(g.pack, vec![axe, bow]);
        assert_eq!(g.hero().max_hp(&g.content), 75);
        let c = g.content.clone();
        g.squad.iter_mut().for_each(|u| u.heal_full(&c));
        let shield_slot = g.hero().items.iter().position(|i| *i == Some(shield)).unwrap();
        g.unequip(0, shield_slot).unwrap();
        assert_eq!(g.hero().hp, 70, "HP capped to the new max");
        assert_eq!(g.pack, vec![axe, bow, shield]);
        assert_eq!(g.unequip(0, shield_slot), Err(EquipError::NoSuchItem));
        g.pack = vec![axe; PACK_SIZE];
        assert_eq!(g.unequip(0, 0), Err(EquipError::PackFull));
    }

    #[test]
    fn an_item_raising_max_hp_brings_its_hit_points() {
        let mut g = quiet_game(HeroClass::Knight);
        let shield = item(&g, "oak_shield");
        let c = g.content.clone();
        g.squad[0].heal_full(&c);
        assert_eq!((g.hero().hp, g.hero().max_hp(&c)), (70, 70));
        g.pack = vec![shield];
        g.equip(0, 0).unwrap();
        assert_eq!((g.hero().hp, g.hero().max_hp(&c)), (75, 75), "healed with the new maximum, not 70/75");
        // Wounded, the HP follows the maximum (0x4908a8): 75 × 60 / 70 = 64.29.
        let slot = g.hero().items.iter().position(|i| *i == Some(shield)).unwrap();
        g.unequip(0, slot).unwrap();
        g.squad[0].hp = 60;
        g.equip(0, 0).unwrap();
        assert_eq!(g.hero().hp, 64);
    }

    #[test]
    fn the_hero_window_puts_an_item_in_the_clicked_slot() {
        // 0x4c24f4: a click on an empty worn slot puts the item in that very slot (its f-
        // values and bonus then win over the slots before it); an occupied one takes nothing.
        let mut g = quiet_game(HeroClass::Knight);
        let shield = item(&g, "oak_shield");
        g.squad[0].items = [None; crate::rules::items::SLOTS];
        g.pack = vec![shield, shield];
        assert_eq!(g.equip_at(0, 0, Some(2)), Ok(()));
        assert_eq!(g.hero().items[2], Some(shield));
        g.squad[0].items[2] = None;
        g.squad[0].items[1] = Some(shield);
        g.pack = vec![shield];
        assert_eq!(g.equip_at(0, 0, Some(1)), Err(EquipError::SameType), "the wear test still runs");
        g.squad[0].items[1] = None;
        g.squad[0].items[3] = Some(item(&g, "short_sword"));
        assert_eq!(g.equip_at(0, 0, Some(3)), Err(EquipError::NoFreeSlot), "an occupied slot does not swap");
        assert_eq!(g.equip(0, 0), Ok(()));
        assert_eq!(g.hero().items[0], Some(shield), "the army card's drop: the lowest free slot");
    }

    #[test]
    fn a_worn_item_is_handed_to_another_unit() {
        let mut g = quiet_game(HeroClass::Knight);
        let (shield, sword) = (item(&g, "oak_shield"), item(&g, "short_sword"));
        let taken: Vec<_> = g.squad.iter().map(|u| u.slot).collect();
        let slot = g.content.formation.free_slot(&taken, crate::rules::formation::Row::Front).unwrap();
        g.squad.push(Unit::new(&g.content, unit(&g, "spearman"), slot));
        g.pack = vec![shield, sword];
        g.equip(0, 0).unwrap();
        g.equip(1, 0).unwrap();
        let at = g.hero().items.iter().position(|i| *i == Some(shield)).unwrap();
        g.give(0, at, 1).unwrap();
        assert!(!g.hero().items.contains(&Some(shield)));
        assert!(g.squad[1].items.contains(&Some(shield)));
        let back = g.squad[1].items.iter().position(|i| *i == Some(shield)).unwrap();
        g.pack = vec![item(&g, "oak_shield")];
        g.equip(0, 0).unwrap();
        assert_eq!(g.give(1, back, 0), Err(EquipError::SameType), "the hero has a shield again");
        assert!(g.squad[1].items.contains(&Some(shield)), "a refused hand-over keeps the item");
        assert_eq!(g.give(0, 3, 1), Err(EquipError::NoSuchItem));
    }

    #[test]
    fn gear_and_potions_go_into_battle_and_potions_wear_off() {
        let mut g = quiet_game(HeroClass::Knight);
        g.pack = vec![item(&g, "might_potion"), item(&g, "chainmail"), item(&g, "heal_potion")];
        g.equip(0, 1).unwrap();
        assert_eq!(g.drink(0, 0), Ok(0), "might: no healing, lasts until the battle ends");
        g.squad[0].hp = 30;
        assert_eq!(g.drink(0, 0), Ok(20));
        g.foe = Some(Foe::Garrison(g.world.index_of("Bandit camp")));
        let mut b = g.start_battle();
        b.begin();
        use crate::rules::content::Stat;
        assert_eq!(b.fighters[0].stats[Stat::DefenceBlow], 13, "10 + chainmail 3");
        assert_eq!(b.fighters[0].stats[Stat::AttackBlow], 28, "24 + might 4");
        wipe_all_but_hero(&mut b);
        g.resolve_battle(&b);
        assert!(g.hero().potions.is_empty());
        let h = g.hero();
        assert_eq!(h.stats(&g.content)[Stat::AttackBlow], h.base_stats(&g.content)[Stat::AttackBlow], "might is gone");
    }

    #[test]
    fn dead_recruits_stay_as_corpses_and_drop_their_gear() {
        let mut g = quiet_game(HeroClass::Knight);
        g.hire(unit(&g, "spearman")).unwrap();
        let mail = item(&g, "chainmail");
        g.squad[1].items[0] = Some(mail);
        let held = Some(crate::rules::units::SpellSlot { spell: 1, until: u64::MAX });
        (g.squad[0].spells[0], g.squad[1].spells[0]) = (held, held);
        g.foe = Some(Foe::Garrison(g.world.index_of("Bandit camp")));
        let mut b = g.start_battle();
        b.begin();
        wipe_all_but_hero(&mut b);
        g.resolve_battle(&b);
        assert_eq!(g.squad.len(), 2);
        assert!(!g.squad[1].alive() && g.squad[1].items[0].is_none());
        assert!(g.pack.contains(&mail), "the dead hold no items");
        // 0x4c50ec: the dead lose their spell slots, the living keep theirs.
        assert_eq!((g.squad[0].spells[0], g.squad[1].spells[0]), (held, None));
    }

    #[test]
    fn loot_that_does_not_fit_is_left_behind() {
        let mut g = quiet_game(HeroClass::Knight);
        g.pack = vec![item(&g, "heal_potion"); PACK_SIZE];
        let lair = g.world.index_of("Bandit lair");
        g.foe = Some(Foe::Garrison(lair));
        let mut b = g.start_battle();
        b.begin();
        wipe_all_but_hero(&mut b);
        assert!(matches!(g.resolve_battle(&b), BattleResult::Victory { left_behind: 2, .. }));
    }

    #[test]
    fn villages_sometimes_pay_in_items() {
        let mut items = 0;
        for seed in 0..200 {
            let mut g = quiet_game(HeroClass::Knight);
            g.rng = Rng::new(seed);
            g.location = Some(g.world.index_of("Millbrook"));
            if let Some(Tribute::Item(item)) = g.collect_tribute() {
                assert!(g.content.sources(item).contains(&Source::Tribute));
                items += 1;
            }
        }
        assert!((25..=80).contains(&items), "about 25%: {items}/200");
    }

    // --- Scenario worlds (hand-built; see `world::testkit`) ---

    use crate::dt::dtm::{BuildingType, Scenario};
    use crate::rules::world::testkit::{self as tk, army, building, hero, scenario, troop};

    /// A 24×6 grass strip; the knight starts at (2, 2) with two warriors and 200 gold.
    #[test]
    fn walking_across_a_friendly_building_does_not_enter_it() {
        let mut s = strip();
        let mut v = building(BuildingType::Village, 10, 3, (2, 2));
        v.relations = [1, 0, 0, 0];
        v.gold_per_day = 25;
        v.gold_max = 50;
        s.buildings = vec![v];
        let mut g = start(&s);
        g.world.armies.clear();
        // No fog: these are about buildings on the route.
        g.fog = crate::rules::fog::Fog::disabled(g.world.map.w, g.world.map.h);
        assert!(g.set_destination((20, 2)));
        assert!(g.path.iter().any(|&t| g.world.location_covering(t) == Some(0)), "the road runs through the village");
        let events = walk_until_stopped(&mut g);
        assert_eq!(g.tile(), (20, 2), "walked on to the point clicked");
        assert!(!events.iter().any(|e| matches!(e, Event::Arrived(_) | Event::Tribute { .. })), "{events:?}");
        assert_eq!(g.world.locations[0].tribute_gold, 25, "passing by takes no tribute");
        // Clicking the village itself enters it.
        assert!(g.set_destination((9, 2)));
        let events = walk_until_stopped(&mut g);
        assert!(events.contains(&Event::Arrived(0)), "{events:?}");
    }

    #[test]
    fn the_route_crosses_even_a_hostile_town() {
        let mut s = strip();
        // A hostile town across rows 0–4 of columns 11–12; row 5 stays open.
        let mut t = building(BuildingType::Town, 12, 4, (2, 5));
        t.relations = [-2, 0, 0, 0];
        t.faction = 4;
        s.buildings = vec![t];
        let mut g = start(&s);
        g.world.armies.clear();
        // No fog: these are about buildings on the route.
        g.fog = crate::rules::fog::Fog::disabled(g.world.map.w, g.world.map.h);
        // The original's planner closes only castles, forts and ruins: the road runs
        // through the town, entered on the way (two cells crossed) but not opened.
        assert!(!g.world.locations[0].bars_hero());
        assert!(g.set_destination((20, 2)));
        assert!(g.path.iter().filter(|&&t| g.world.location_covering(t) == Some(0)).count() == 2, "{:?}", g.path);
        let events = walk_until_stopped(&mut g);
        assert_eq!((g.tile(), g.location), ((20, 2), None));
        assert!(!events.iter().any(|e| matches!(e, Event::Arrived(_))));
        // Clicked, it is the destination: the walk ends on the cell clicked, and it opens.
        assert!(g.set_destination((11, 2)));
        assert_eq!(g.path.last(), Some(&(11, 2)));
        let events = walk_until_stopped(&mut g);
        assert_eq!(events.last(), Some(&Event::Arrived(0)));
    }

    fn strip() -> Scenario {
        let mut s = scenario(24, 6);
        s.header.heroes[0] = hero(2, 2, 200, &[troop(4, 0, 2)]);
        s.header.heroes[0].artifacts = [7, 0, 0];
        s
    }

    fn start(s: &Scenario) -> Game {
        Game::from_scenario(Arc::new(tk::content()), s, HeroClass::Knight)
    }

    fn wipe_enemies(b: &mut Battle) {
        for f in b.fighters.iter_mut().filter(|f| f.team == Team::Enemy) {
            f.hp = 0;
        }
    }

    #[test]
    fn scenario_game_starts_from_the_preset() {
        let g = start(&strip());
        assert_eq!((g.tile(), g.gold, g.mana, g.squad.len()), ((2, 2), 200, 0, 3));
        assert_eq!((g.hero().level, g.hero().xp), (1, 0), "no starting XP in the preset");
        let mut s = strip();
        s.header.heroes[0].mana = 150;
        assert_eq!(start(&s).mana, 150, "the preset's second value is mana");
        assert_eq!(g.hero().def, HeroClass::Knight.unit());
        assert_eq!(g.pack, vec![ItemId(7)]);
        assert_eq!(g.clock.label(), "1204, month 5, day 19, 9 h");
        assert!(!g.won(), "no camps: not won by clearing them");
    }

    #[test]
    fn hostile_armies_attack_on_contact_and_leave_loot() {
        let mut s = strip();
        let mut foe = army(1, 12, 2, -2, &[troop(4, 0, 2), troop(5, 0, 1)]);
        foe.gold_income = 120;
        foe.artifacts = [9, 0, 0];
        s.armies = vec![foe];
        let mut g = start(&s);
        g.fog = Fog::disabled(24, 6);
        // Walking onto it (§4.2): a battle.
        let at = g.world.armies[0].tile(&g.world.map);
        assert!(g.set_destination(at));
        let events = walk_until_stopped(&mut g);
        assert_eq!(events.last(), Some(&Event::Encounter(0)));
        assert_eq!(g.foe, Some(Foe::Army(0)));
        assert!(g.world.map.distance(g.tile(), g.world.armies[0].tile(&g.world.map)) <= 1);
        let mut b = g.start_battle();
        assert_eq!(b.fighters.iter().filter(|f| f.team == Team::Enemy).count(), 3);
        b.begin();
        wipe_enemies(&mut b);
        let gold = g.gold;
        // 120 to start with (word 17 is starting gold); a feudal army pays its wages at noon.
        let carried = g.world.armies[0].gold;
        assert!(carried > 0 && carried <= 120, "{carried}");
        // Half its gold (no minimum) and its daily wages.
        let wages = crate::rules::ai::army_wages(&g.content, &g.world.armies[0].troops);
        assert!(wages > 0);
        let r = g.resolve_battle(&b);
        assert!(matches!(&r, BattleResult::Victory { reward, captured: None, loot, .. } if *reward == carried / 2 + wages && loot == &vec![ItemId(9)]), "{r:?}");
        assert_eq!(g.gold, gold + carried / 2 + wages);
        assert!(g.world.armies.is_empty());
    }

    /// A strip game with army 1 (attitude `attitude`) at `at` walking `path` by itself (its
    /// AI off), no fog.
    fn with_walker(attitude: i8, at: Tile, path: Vec<Tile>) -> Game {
        let mut s = strip();
        s.armies = vec![army(1, at.0 as u16, at.1 as u16, attitude, &[troop(4, 0, 1)])];
        let mut g = start(&s);
        g.fog = Fog::disabled(24, 6);
        let a = &mut g.world.armies[0];
        a.mind.scripted = true;
        a.path = path;
        g
    }

    #[test]
    fn a_friendly_army_stepping_next_to_the_hero_greets_him_then_not_for_long() {
        // World.md §4.3: the army steps (5, 2) → (4, 2) during his diagonal step to (3, 3):
        // next to him, its talk counter 0 + 1 + (attitude 1 + 1) = 3 is above 0: a meeting,
        // and the counter falls to −500.
        let mut g = with_walker(1, (5, 2), vec![(4, 2)]);
        assert!(g.set_destination((3, 3)));
        let events = walk_until_stopped(&mut g);
        assert_eq!(events.last(), Some(&Event::Met(0)));
        assert_eq!((g.foe, g.world.armies[0].talk), (None, -500));
        // Again next to him after a step of its own: the counter is still far below 0.
        g.world.armies[0].path = vec![(4, 3)];
        assert!(g.set_destination((3, 4)));
        let events = walk_until_stopped(&mut g);
        assert!(!events.iter().any(|e| matches!(e, Event::Met(_) | Event::Encounter(_))), "{events:?}");
        assert_eq!(g.world.armies[0].talk, -500 + 1 + 2);
        // An army with no path steps in place on its own cell's cost (0x4a399c): each of
        // those is an arrival too, so it greets him as well. A stationary guard never does.
        let mut g = with_walker(1, (4, 2), vec![]);
        assert!(g.set_destination((3, 3)));
        let events = walk_until_stopped(&mut g);
        assert!(events.contains(&Event::Met(0)), "{events:?}");
        let mut g = with_walker(1, (4, 2), vec![]);
        g.world.armies[0].patrols = true;
        g.world.armies[0].patrol_radius = 0;
        assert!(g.set_destination((3, 3)));
        let events = walk_until_stopped(&mut g);
        assert!(!events.iter().any(|e| matches!(e, Event::Met(_))), "{events:?}");
    }

    #[test]
    fn talk_counters_grow_by_the_relation_wherever_the_hero_is() {
        // 0x4a399c, 0x4a548c: each step of an army adds 1 (it is not on his cell) and, when
        // its relation to him is 0 or above, relation + 1, far from him too.
        let mut g = with_walker(1, (12, 2), vec![(13, 2), (14, 2)]);
        let rel = g.world.armies[0].attitude as i32;
        assert!(rel >= 0);
        g.wait(1);
        assert_eq!(g.world.armies[0].tile(&g.world.map), (14, 2));
        assert_eq!(g.world.armies[0].talk, 2 * (1 + rel + 1));
        let mut g = with_walker(-2, (12, 2), vec![(13, 2), (14, 2)]);
        g.wait(1);
        assert_eq!(g.world.armies[0].talk, 2, "ill-disposed: only the 1 per step");
    }

    #[test]
    fn a_greeting_without_an_event_does_not_stop_the_walk() {
        // 0x4ade3c: a greeting runs the events with the army; only one that fires stops him.
        // It steps (5, 2) → (4, 2) during his first step, to (3, 3): next to him, it greets
        // him, and he walks on to (8, 4).
        let mut g = with_walker(1, (5, 2), vec![(4, 2)]);
        assert!(g.set_destination((8, 4)));
        let events = walk_until_stopped(&mut g);
        assert!(events.contains(&Event::Met(0)), "{events:?}");
        // Its counter fell to −500 and grows again by 1 + 2 at each step in place since.
        let talk = g.world.armies[0].talk;
        assert_eq!(g.tile(), (8, 4));
        assert!((-500..0).contains(&talk) && (talk + 500) % 3 == 0, "{talk}");
    }

    #[test]
    fn of_two_armies_next_to_him_the_last_in_order_acts() {
        // 0x4ade3c keeps overwriting its pick in the army loop: the last one greets.
        let mut s = strip();
        s.armies = vec![army(1, 5, 2, 1, &[troop(4, 0, 1)]), army(2, 5, 4, 1, &[troop(4, 0, 1)])];
        let mut g = start(&s);
        g.fog = Fog::disabled(24, 6);
        for (a, to) in g.world.armies.iter_mut().zip([(4, 2), (4, 4)]) {
            a.mind.scripted = true;
            a.path = vec![to];
        }
        assert!(g.set_destination((3, 3)));
        let events = walk_until_stopped(&mut g);
        assert!(events.contains(&Event::Met(1)) && !events.contains(&Event::Met(0)), "{events:?}");
        // Both greeted him in their arrivals (0x4a548c): both counters fell to −500.
        assert_eq!((g.world.armies[0].talk, g.world.armies[1].talk), (-500, -500));
    }

    #[test]
    fn armies_keep_off_the_cell_ahead_of_a_standing_hero() {
        // The original keeps the hero's direction after a walk (0x75c050), and an AI step
        // onto his cell plus that direction stays put (0x4a399c). He walks east to (4, 2):
        // (5, 2) ahead of him is closed while he waits, (5, 3) is not.
        let mut g = with_walker(1, (6, 2), vec![]);
        assert!(g.set_destination((4, 2)));
        walk_until_stopped(&mut g);
        assert_eq!((g.facing, g.world.armies[0].tile(&g.world.map)), (Some((1, 0)), (6, 2)));
        g.world.armies[0].path = vec![(5, 2)];
        g.wait(1);
        assert_eq!(g.world.armies[0].tile(&g.world.map), (6, 2), "the cell ahead of him");
        g.world.armies[0].path = vec![(5, 3)];
        g.wait(1);
        assert_eq!(g.world.armies[0].tile(&g.world.map), (5, 3));
    }

    #[test]
    fn a_chased_army_gone_into_the_dark_ends_the_chase() {
        // 0x4aedd1 tests the army's cell once the fog is laid over the mask: it runs east,
        // five cells for each of his steps, out of his sight after his first step.
        let mut g = with_walker(1, (8, 2), (9..=16).map(|x| (x, 2)).collect());
        g.world.armies[0].speed = 1;
        g.fog = Fog::new(24, 6);
        g.look_around();
        assert!(g.set_destination((8, 2)));
        walk_until_stopped(&mut g);
        assert!(!g.fog.explored(g.world.armies[0].tile(&g.world.map)));
        assert_eq!((g.tile(), g.talk_to, g.moving()), ((3, 2), None, false));
    }

    #[test]
    fn ai_armies_keep_off_the_heros_cells_and_attack_only_after_his_step() {
        // A hostile army two cells east walks west along row 2: it may not enter his cell,
        // so it stays next to him; it attacks after his step, never while he waits.
        let mut g = with_walker(-2, (4, 2), vec![(3, 2), (2, 2), (1, 2)]);
        // Its cached battle score against him says it wins (ai.md §8).
        g.world.armies[0].mind.scores.insert(ai::HERO, 1);
        g.wait(2);
        assert_eq!(g.world.armies[0].tile(&g.world.map), (3, 2), "it stopped short of him");
        assert!(g.foe.is_none(), "no attack while he waits");
        assert!(g.world.armies[0].path.first() == Some(&(2, 2)), "it keeps trying");
        assert!(g.set_destination((2, 3)));
        let events = walk_until_stopped(&mut g);
        assert_eq!(events.last(), Some(&Event::Encounter(0)));
        assert_eq!(g.foe, Some(Foe::Army(0)));
        // A stationary guard (patrol radius 0) does not even bank time.
        let mut g = with_walker(-2, (4, 2), vec![(3, 2)]);
        g.world.armies[0].patrols = true;
        g.world.armies[0].patrol_radius = 0;
        g.wait(4);
        assert_eq!((g.world.armies[0].tile(&g.world.map), g.world.armies[0].budget), ((4, 2), 0.0));
    }

    #[test]
    fn the_hero_hides_from_an_attacker_in_someone_elses_building() {
        // ai.md §8: a hostile army next to him attacks only when he stands in no building, on
        // a bridge or in his own; in a town or market not his, nothing happens (0x4a548c).
        for kind in [BuildingType::Town, BuildingType::Market] {
            let mut s = strip();
            s.buildings = vec![building(kind, 2, 3, (1, 1))];
            s.armies = vec![army(1, 4, 2, -2, &[troop(4, 0, 1)])];
            let mut g = start(&s);
            g.fog = Fog::disabled(24, 6);
            g.world.armies[0].mind.scripted = true;
            g.world.armies[0].path = vec![(3, 2), (2, 2), (1, 2)];
            g.world.armies[0].mind.scores.insert(ai::HERO, 1);
            g.wait(2);
            assert_eq!(g.world.armies[0].tile(&g.world.map), (3, 2));
            assert!(g.set_destination((2, 3)));
            let events = walk_until_stopped(&mut g);
            assert!(!events.iter().any(|e| matches!(e, Event::Encounter(_))), "{kind:?}: {events:?}");
            assert_eq!((g.tile(), g.foe), ((2, 3), None), "{kind:?}");
            g.wait(4);
            assert!(g.foe.is_none(), "{kind:?}: still hidden");
        }
    }

    #[test]
    fn an_attack_is_shown_after_the_step_that_brought_it_has_played() {
        // The original's armies walk while the hero's step plays and their attack comes at
        // its end (0x4ade3c): on screen the step's window plays out, hero and attacker drawn
        // onto their cells, before the attack's events (and the battle) come.
        let mut g = with_walker(-2, (4, 2), vec![(3, 2), (2, 2), (1, 2)]);
        g.world.armies[0].mind.scores.insert(ai::HERO, 1);
        g.wait(2);
        assert!(g.set_destination((2, 3)));
        let mut set_at = None;
        for frame in 0..1000 {
            let events = g.tick_shown(0.01);
            if g.foe.is_some() && set_at.is_none() {
                set_at = Some(frame);
                assert!(g.step_playing(), "the attack's step has its window to play");
                assert!(events.is_empty(), "its events wait for it");
            }
            if events.contains(&Event::Encounter(0)) {
                assert!(frame > set_at.unwrap(), "shown after the step");
                assert!(!g.step_playing());
                assert_eq!(g.display_pos(), g.pos, "the hero drawn on his cell");
                let a = &g.world.armies[0];
                assert_eq!(g.army_display_pos(a), a.pos, "the attacker drawn on its cell");
                return;
            }
        }
        panic!("no attack");
    }

    #[test]
    fn a_wait_tick_does_not_replay_the_heros_last_step() {
        // A wait's ticks restart the drawing window: the hero stays on his cell (the view
        // follows him, so a replayed step shook the map every tick).
        let mut g = with_walker(-2, (20, 2), vec![]);
        g.fog = Fog::disabled(g.world.map.w, g.world.map.h);
        assert!(g.set_destination((3, 2)));
        walk_until_stopped(&mut g);
        g.since_step = STEP_SECONDS * 2.0;
        g.begin_wait(1);
        g.tick(STEP_SECONDS);
        assert!(g.since_step < STEP_SECONDS, "a tick ran");
        assert_eq!(g.display_pos(), g.pos);
    }

    #[test]
    fn the_hero_is_drawn_along_his_step_with_the_armies() {
        // His step plays over the window after it was taken, from the cell he left.
        let mut g = with_walker(-2, (20, 2), vec![]);
        g.fog = Fog::disabled(g.world.map.w, g.world.map.h);
        let start = g.pos;
        assert!(g.set_destination((3, 2)));
        g.tick(STEP_SECONDS);
        assert_ne!(g.pos, start, "the first step is taken");
        assert_eq!(g.display_pos(), start, "and drawn from where he stood");
        g.since_step = STEP_SECONDS / 2.0;
        let mid = g.display_pos();
        assert!((mid.0 - (start.0 + g.pos.0) / 2.0).abs() < 1e-3 && (mid.1 - (start.1 + g.pos.1) / 2.0).abs() < 1e-3);
        g.since_step = STEP_SECONDS;
        assert_eq!(g.display_pos(), g.pos);
    }

    #[test]
    fn stepping_onto_an_army_engages_it() {
        // §4.2: the cell he is about to enter holds an army: he stays where he is and it is
        // engaged, friendly or not: with no event firing, a battle.
        let mut g = with_walker(-2, (5, 2), vec![]);
        // A stationary guard: it does not come for him.
        g.world.armies[0].patrols = true;
        g.world.armies[0].patrol_radius = 0;
        assert!(g.set_destination((5, 2)), "the army clicked");
        let events = walk_until_stopped(&mut g);
        assert_eq!((events.last(), g.tile(), g.foe), (Some(&Event::Encounter(0)), (4, 2), Some(Foe::Army(0))));
        let mut g = with_walker(2, (5, 2), vec![]);
        assert!(g.set_destination((5, 2)));
        let events = walk_until_stopped(&mut g);
        assert_eq!((events.last(), g.tile(), g.foe), (Some(&Event::Encounter(0)), (4, 2), Some(Foe::Army(0))), "a friend too, as the original");
    }

    #[test]
    fn the_hero_follows_the_army_he_clicked_until_they_meet() {
        // It walks away east; after each of its steps his route is planned again to its new
        // cell (0x4aedd1), until he steps onto it.
        let mut g = with_walker(1, (8, 2), (9..=14).map(|x| (x, 2)).collect());
        g.world.armies[0].speed = 10;
        assert!(g.set_destination((8, 2)));
        let events = walk_until_stopped(&mut g);
        assert!(events.contains(&Event::Encounter(0)), "engaged where it went: {events:?}");
        let at = g.world.armies[0].tile(&g.world.map);
        assert!(at.0 > 8 && g.tile() == (at.0 - 1, 2), "next to it: {:?} {at:?}", g.tile());
        // Moved by other means (not a step of its own), it is not followed: he walks to the
        // cell clicked. (A stationary guard: an army with no path steps in place.)
        let mut g = with_walker(1, (8, 2), vec![]);
        g.world.armies[0].patrols = true;
        g.world.armies[0].patrol_radius = 0;
        assert!(g.set_destination((8, 2)));
        g.world.armies[0].pos = g.world.map.center((12, 2));
        walk_until_stopped(&mut g);
        assert_eq!(g.tile(), (8, 2));
    }

    #[test]
    fn a_chased_army_out_of_reach_ends_the_chase_and_stops_the_hero() {
        // It walks into a hostile castle. The new plan keeps the buildings of the original
        // click (0x4aedd1): the castle is not the one clicked, so it is closed, the army's
        // cell with it, and the hero stops where he is.
        let mut s = strip();
        let mut castle = building(BuildingType::Castle, 10, 2, (1, 1));
        castle.relations = [-2, 0, 0, 0];
        s.buildings = vec![castle];
        s.armies = vec![army(1, 8, 2, 1, &[troop(4, 0, 1)])];
        let mut g = start(&s);
        g.fog = Fog::disabled(24, 6);
        let a = &mut g.world.armies[0];
        a.mind.scripted = true;
        a.path = vec![(9, 2), (10, 2)];
        a.speed = 10;
        assert!(g.set_destination((8, 2)));
        walk_until_stopped(&mut g);
        assert_eq!(g.world.armies[0].tile(&g.world.map), (10, 2));
        assert!(g.talk_to.is_none() && !g.moving() && g.foe.is_none());
        assert!(g.tile().0 < 9, "stopped on the way at {:?}", g.tile());
    }

    #[test]
    fn clicking_a_friendly_army_meets_it_again() {
        // The help: "click it to talk or fight": stepping onto it engages it, whatever its
        // talk counter: with no event firing, a battle each time, friendly or not.
        let mut g = with_walker(1, (10, 2), vec![]);
        for _ in 0..2 {
            let at = g.world.armies[0].tile(&g.world.map);
            assert!(g.set_destination(at));
            let events = walk_until_stopped(&mut g);
            assert!(events.contains(&Event::Encounter(0)), "{events:?}");
            assert_eq!((g.foe, g.tile()), (Some(Foe::Army(0)), (9, 2)));
            g.foe = None;
            g.pos = g.world.map.center((4, 2));
        }
    }

    #[test]
    fn hostile_armies_chase_a_nearby_hero() {
        let mut s = strip();
        let mut foe = army(1, 7, 4, -2, &[troop(4, 0, 1)]);
        foe.patrols = 0;
        // The AI goes only for battles it would win: a bold one for this; and no wandering.
        foe.aggression = 100;
        foe.no_random_targets = 1;
        // Gold for its wages: its noon marks its pairs dirty, and an unpaid crew scores no
        // battle (ai.md §4).
        foe.gold_income = 500;
        s.armies = vec![foe];
        let mut g = start(&s);
        let events = g.wait(4);
        assert!(!events.iter().any(|e| matches!(e, Event::Encounter(_))), "no attack on a waiting hero: {events:?}");
        let a = g.world.armies[0].tile(&g.world.map);
        assert!((a.0 - 2).abs() <= 1 && (a.1 - 2).abs() <= 1, "it came next to him: {a:?}");
        // His next step, to a cell still next to it: it steps after him (onto his cell,
        // which it never enters) and attacks.
        let n = g.world.map.grid.neighbours((2, 2)).find(|&n| n != a && (n.0 - a.0).abs() <= 1 && (n.1 - a.1).abs() <= 1 && g.world.map.passable(n)).unwrap();
        assert!(g.set_destination(n));
        let events = walk_until_stopped(&mut g);
        assert!(matches!(events.last(), Some(Event::Encounter(0))), "{events:?}");
    }

    #[test]
    fn fighting_in_his_own_building_gives_the_heros_side_its_defence() {
        // battle.md §0 (485908): every unit's defence gets + building defence, for the side in
        // its own building; the footage's panel: "in its own building, a bonus to all defences".
        let mut s = strip();
        let mut castle = building(BuildingType::Castle, 6, 3, (2, 2));
        castle.garrison_extra_defence = 12;
        s.buildings = vec![castle];
        s.armies = vec![army(1, 12, 2, -2, &[troop(4, 0, 1)])];
        let mut g = start(&s);
        g.world.locations[0].owner = crate::rules::world::Owner::Player;
        // Standing outside: no bonus.
        g.foe = Some(Foe::Army(0));
        assert_eq!(g.start_battle().building_defence(Team::Player), 0);
        // In his own castle: its extra defence for every unit of his.
        g.location = Some(0);
        let b = g.start_battle();
        assert_eq!(b.building_defence(Team::Player), 12);
        assert_eq!(b.building_defence(Team::Enemy), 0, "the attackers stand outside");
    }

    #[test]
    fn a_friends_building_helps_the_hero_and_an_enemys_helps_the_enemy() {
        let mut s = strip();
        let mut castle = building(BuildingType::Castle, 6, 3, (2, 2));
        castle.garrison_extra_defence = 12;
        s.buildings = vec![castle];
        s.armies = vec![army(1, 12, 2, -2, &[troop(4, 0, 1)])];
        let mut g = start(&s);
        g.world.locations[0].owner = crate::rules::world::Owner::Neutral;
        g.world.locations[0].attitude = 2;
        g.foe = Some(Foe::Army(0));
        g.location = Some(0);
        assert_eq!(g.start_battle().building_defence(Team::Player), 12, "a friend's castle");
        g.world.locations[0].attitude = -3;
        g.location = None;
        let at = g.world.locations[0].tile;
        g.world.armies[0].pos = g.world.map.center(at);
        let b = g.start_battle();
        assert_eq!((b.building_defence(Team::Enemy), b.building_defence(Team::Player)), (12, 0), "the enemy at home in a hostile castle");
    }

    #[test]
    fn a_hostile_fort_is_taken_by_beating_its_garrison() {
        let mut s = strip();
        let mut fort = building(BuildingType::Fort, 16, 3, (2, 2));
        fort.faction = 4;
        fort.relations = [-2, 0, 0, 0];
        fort.gold_per_day = 40;
        fort.mana_per_day = 5;
        fort.garrison[0] = troop(4, 0, 2);
        fort.garrison_extra_defence = 12;
        s.buildings = vec![fort];
        let mut g = start(&s);
        g.fog = Fog::disabled(24, 6);
        assert!(g.set_destination((15, 2)), "any cell of it can be clicked");
        assert_eq!(g.goal, Some((15, 2)));
        // §4.2: stepping onto its cell engages the garrison; he stays outside.
        let events = walk_until_stopped(&mut g);
        assert_eq!(events.last(), Some(&Event::Arrived(0)));
        assert_eq!((g.foe, g.tile(), g.location), (Some(Foe::Garrison(0)), (14, 2), None));
        assert_eq!(g.daily_income(), 0);
        let mut b = g.start_battle();
        assert_eq!(b.attacker, Team::Player);
        b.begin();
        wipe_enemies(&mut b);
        assert!(matches!(g.resolve_battle(&b), BattleResult::Victory { captured: Some(0), .. }));
        let fort = &g.world.locations[0];
        assert!(fort.owned() && !fort.defended() && fort.garrison.is_empty());
        // The shown income ×F/100 for the player (F = 120 without "impossible difficulty").
        // With no maximum its stock never grows, so the noon pays nothing from it, and no
        // building pays mana at noon (economy.md §1, §3).
        assert_eq!(g.daily_income(), 48);
        let (gold, mana, wages) = (g.gold, g.mana, g.daily_wages());
        let mut events = Vec::new();
        g.pass_time(24.0 * 60.0, &mut events);
        assert!(matches!(events.as_slice(), [Event::NewDay(DayReport { income: 48, .. })]), "{events:?}");
        assert_eq!(g.mana, mana);
        assert_eq!(g.gold, (gold - wages).max(0));
    }

    #[test]
    fn beating_any_garrison_makes_the_place_the_heros() {
        // Ruins (like castles and forts): once their guards are beaten the place is his.
        let mut s = strip();
        let mut ruins = building(BuildingType::Ruins, 16, 3, (2, 2));
        ruins.faction = 4;
        ruins.relations = [-2, 0, 0, 0];
        ruins.garrison[0] = troop(4, 0, 2);
        s.buildings = vec![ruins];
        let mut g = start(&s);
        g.fog = Fog::disabled(24, 6);
        g.set_destination(g.world.locations[0].tile);
        walk_until_stopped(&mut g);
        assert_eq!(g.foe, Some(Foe::Garrison(0)));
        let mut b = g.start_battle();
        b.begin();
        wipe_enemies(&mut b);
        assert!(matches!(g.resolve_battle(&b), BattleResult::Victory { captured: Some(0), .. }));
        let ruins = &g.world.locations[0];
        assert!(ruins.owned() && !ruins.defended() && !ruins.hostile(), "the ruins are his now");
    }

    #[test]
    fn the_fog_lifts_around_the_walking_hero() {
        let mut g = start(&strip());
        g.world.armies.clear();
        // The knight sees 9 cells.
        assert!(g.fog.enabled && g.fog.explored((2, 2)) && g.fog.explored((11, 2)) && !g.fog.explored((12, 2)));
        assert!(!Game::new(content(), HeroClass::Knight).fog.enabled, "the demo has no fog");
        // A click into the dark is no target (world.md §1.3): nothing happens.
        assert!(!g.can_target((22, 3)) && g.route_to((22, 3)).is_empty());
        assert!(!g.set_destination((22, 3)) && !g.moving());
        assert!(g.set_destination((11, 3)));
        assert!(g.path.iter().all(|&t| g.fog.explored(t)), "over explored ground only");
        walk_until_stopped(&mut g);
        assert_eq!(g.tile(), (11, 3));
        assert!(g.fog.explored((20, 3)) && g.can_target((20, 3)), "new ground in sight");
        assert!(g.set_destination((20, 3)));
        walk_until_stopped(&mut g);
        assert!(g.fog.explored((23, 0)) && g.goal.is_none());
        g.set_destination((2, 2));
        g.stop();
        assert!(g.goal.is_none() && !g.moving());
        let mut g = start(&strip());
        assert!(!g.fog.explored((21, 5)));
        g.reveal(20, 5, 2);
        assert!(g.fog.explored((21, 5)) && g.fog.explored((22, 5)) && !g.fog.explored((23, 5)));
    }

    #[test]
    fn a_hostile_fort_bars_the_route_unless_clicked_or_stood_in() {
        let mut s = strip();
        let mut fort = building(BuildingType::Fort, 10, 2, (1, 1));
        fort.relations = [-2, 0, 0, 0];
        fort.garrison[0] = troop(4, 0, 1);
        s.buildings = vec![fort];
        // Water above and below the fort: the only way east is through it.
        for y in [0u32, 1, 3, 4, 5] {
            tk::set(&mut s, 10, y, crate::dt::dtm::Surface::DeepSea);
        }
        let mut g = start(&s);
        g.fog = Fog::disabled(24, 6);
        assert!(g.world.locations[0].bars_hero());
        assert!(!g.set_destination((20, 2)), "no route through an ill-disposed fort");
        assert!(g.set_destination((10, 2)), "the fort itself can be clicked");
        let events = walk_until_stopped(&mut g);
        assert_eq!(events.last(), Some(&Event::Arrived(0)));
        assert_eq!((g.tile(), g.foe), ((9, 2), Some(Foe::Garrison(0))), "its garrison meets him at the gate");
        let mut b = g.start_battle();
        b.begin();
        wipe_enemies(&mut b);
        g.resolve_battle(&b);
        assert!(!g.world.locations[0].bars_hero(), "taken: his own");
        assert!(g.set_destination((20, 2)));
        // A neutral fort (attitude 0, no garrison) bars the way too: attitude 0 or less. Clicked,
        // it is taken on the first step onto it (§4.2: an empty garrison).
        let mut s2 = s.clone();
        s2.buildings[0].relations = [0, 0, 0, 0];
        s2.buildings[0].garrison[0] = troop(0, 0, 0);
        let mut g = start(&s2);
        g.fog = Fog::disabled(24, 6);
        assert!(!g.set_destination((20, 2)));
        assert!(g.set_destination((10, 2)));
        let events = walk_until_stopped(&mut g);
        assert!(events.contains(&Event::Captured(0)), "{events:?}");
        assert_eq!((g.location, g.foe), (Some(0), None));
        assert!(g.world.locations[0].owned());
        assert!(g.set_destination((20, 2)), "from inside it he walks on");
    }

    #[test]
    fn towns_and_villages_on_the_way_are_crossed_without_a_visit() {
        let mut s = strip();
        // A 3 × 3 village across the road east (x 9..=11, y 1..=3).
        let mut v = building(BuildingType::Village, 11, 3, (3, 3));
        v.relations = [1, 0, 0, 0];
        v.gold_per_day = 25;
        v.gold_max = 50;
        s.buildings = vec![v];
        let mut g = start(&s);
        g.fog = Fog::disabled(24, 6);
        let l = &g.world.locations[0];
        assert!(!l.bars_hero() && l.cells().all(|t| g.world.map.cost(t) == Some(crate::rules::map::ROAD)));
        // Walking east along row 2 crosses the village's cells (road): no window, no
        // tribute; but an unguarded village stepped on is his (§4.2, 0x4ad94c).
        assert!(g.set_destination((20, 2)));
        assert!(g.path.iter().any(|&t| g.world.location_at(t) == Some(0)), "the route may cross it");
        let events = walk_until_stopped(&mut g);
        assert!(!events.iter().any(|e| matches!(e, Event::Arrived(_) | Event::Tribute { .. })), "{events:?}");
        assert!(events.contains(&Event::Captured(0)), "{events:?}");
        assert!(g.world.locations[0].owned());
        assert_eq!(g.world.locations[0].tribute_gold, 25, "passing by takes no tribute");
        assert_eq!((g.tile(), g.location), ((20, 2), None));
    }

    #[test]
    fn a_building_is_entered_on_its_second_cell_and_opens_where_the_walk_ends() {
        let mut s = strip();
        // A 3 × 1 town on row 2, x 9..=11 (wider than tall: one more row above, row 1).
        let mut t = building(BuildingType::Town, 11, 2, (3, 1));
        t.relations = [1, 0, 0, 0];
        s.buildings = vec![t];
        let mut g = start(&s);
        g.fog = Fog::disabled(24, 6);
        assert!(g.set_destination((9, 2)));
        walk_until_stopped(&mut g);
        assert_eq!(g.location, Some(0), "the walk ended on it: entered");
        let mut g = start(&s);
        g.fog = Fog::disabled(24, 6);
        assert!(g.set_destination((11, 2)));
        assert_eq!(g.path[..], [(3, 2), (4, 2), (5, 2), (6, 2), (7, 2), (8, 2), (9, 2), (10, 2), (11, 2)]);
        // The first footprint cell, from outside: not entered yet.
        while g.tile() != (9, 2) {
            g.tick(STEP_SECONDS);
        }
        assert_eq!(g.location, None);
        g.tick(STEP_SECONDS);
        assert_eq!((g.tile(), g.location), ((10, 2), Some(0)), "the second: entered, its window not open");
        let events = walk_until_stopped(&mut g);
        assert_eq!(events.last(), Some(&Event::Arrived(0)));
    }

    #[test]
    fn each_step_plays_over_150_ms_and_costs_the_cell_left() {
        let mut s = strip();
        tk::set(&mut s, 2, 2, crate::dt::dtm::Surface::Road);
        tk::set(&mut s, 3, 2, crate::dt::dtm::Surface::Marsh);
        // Water either side of the marsh: the way runs through it.
        tk::set(&mut s, 3, 1, crate::dt::dtm::Surface::DeepSea);
        tk::set(&mut s, 3, 3, crate::dt::dtm::Surface::DeepSea);
        let mut g = start(&s);
        assert!(g.set_destination((5, 2)));
        assert_eq!(g.path, vec![(3, 2), (4, 2), (5, 2)]);
        let t0 = g.clock.total_minutes();
        g.tick(STEP_SECONDS * 0.9);
        assert_eq!((g.tile(), g.clock.total_minutes()), ((2, 2), t0), "the step is still playing");
        g.tick(STEP_SECONDS * 0.2);
        // Leaving the road: 3 × 5 = 15 minutes.
        assert_eq!((g.tile(), g.clock.total_minutes()), ((3, 2), t0 + 15.0));
        g.tick(STEP_SECONDS);
        // Leaving the marsh: 8 × 5 = 40 minutes, though the grass entered costs 25.
        assert_eq!((g.tile(), g.clock.total_minutes()), ((4, 2), t0 + 55.0));
        // A diagonal step is 1.5 times as long; the ranger is 4/5 as slow.
        assert_eq!(g.step_time((4, 2), (5, 3)), 37.5);
        let mut s = strip();
        s.header.heroes[2] = s.header.heroes[0].clone();
        let r = Game::from_scenario(Arc::new(tk::content()), &s, HeroClass::Ranger);
        assert_eq!((r.hero_speed(), r.step_time((4, 2), (5, 2)), r.step_time((4, 2), (5, 3))), (4, 20.0, 30.0));
        assert_eq!(r.sight_radius(), 10);
    }

    /// The map load sets the class's speed (0x4b4300) before it puts the hero on his cell
    /// (0x4b5913 → 0x497c68), so the ranger's first step is priced at his speed 4 too.
    #[test]
    fn the_rangers_first_step_is_priced_at_his_speed() {
        let mut s = strip();
        s.header.heroes[2] = s.header.heroes[0].clone();
        let mut r = Game::from_scenario(Arc::new(tk::content()), &s, HeroClass::Ranger);
        r.world.armies.clear();
        r.pending.clear();
        let cost = u32::from(r.world.map.cost((2, 2)).unwrap());
        assert_eq!(r.step_base, Some(cost * 4));
        assert!(r.set_destination((3, 2)));
        let t0 = r.clock.total_minutes();
        walk_until_stopped(&mut r);
        assert_eq!(r.clock.total_minutes() - t0, f64::from(cost * 4));
    }

    #[test]
    fn waiting_is_ticks_of_half_an_hour_played_in_real_time() {
        let mut g = start(&strip());
        let t0 = g.clock.total_minutes();
        g.wait(1);
        assert_eq!(g.clock.total_minutes(), t0 + 60.0, "1 h = 2 ticks");
        g.begin_wait(4);
        assert!(g.waiting());
        g.tick(STEP_SECONDS * 3.5);
        assert_eq!(g.clock.total_minutes(), t0 + 60.0 + 90.0, "three ticks so far");
        for _ in 0..10 {
            g.tick(STEP_SECONDS);
        }
        assert_eq!(g.clock.total_minutes(), t0 + 60.0 + 240.0, "4 h = 8 ticks");
        assert!(!g.waiting());
        g.tick(1.0);
        assert_eq!(g.clock.total_minutes(), t0 + 300.0, "then time stands still");
    }

    /// A click or a key during a wait (Razdor's choice): the half hour under way plays
    /// out, then the wait ends; the endless wait alike.
    #[test]
    fn a_cut_wait_ends_after_the_tick_under_way() {
        let mut g = start(&strip());
        let t0 = g.clock.total_minutes();
        g.begin_wait(4);
        g.tick(STEP_SECONDS * 2.5);
        assert_eq!(g.clock.total_minutes(), t0 + 60.0, "two ticks played, the third under way");
        g.cut_wait();
        assert!(g.waiting(), "the tick under way still plays");
        g.tick(STEP_SECONDS * 0.6);
        assert_eq!(g.clock.total_minutes(), t0 + 90.0);
        assert!(!g.waiting());
        g.tick(1.0);
        assert_eq!(g.clock.total_minutes(), t0 + 90.0, "the wait is over");
        // The endless wait: the same.
        g.begin_endless_wait();
        g.tick(STEP_SECONDS * 3.5);
        g.cut_wait();
        assert!(!g.endless_waiting() && g.waiting());
        for _ in 0..5 {
            g.tick(STEP_SECONDS);
        }
        assert_eq!(g.clock.total_minutes(), t0 + 90.0 + 4.0 * 30.0);
        assert!(!g.waiting());
        // Nothing to cut: a walk is not touched.
        assert!(g.set_destination((5, 2)));
        let path = g.path.clone();
        g.cut_wait();
        assert_eq!(g.path, path);
    }

    #[test]
    fn the_endless_wait_ticks_until_ended() {
        let mut g = start(&strip());
        let t0 = g.clock.total_minutes();
        g.begin_endless_wait();
        assert!(g.waiting() && g.endless_waiting());
        // F5 before its first tick: the wait of one tick still plays that tick.
        g.end_endless_wait();
        g.tick(STEP_SECONDS * 0.5);
        assert!(g.waiting());
        g.tick(STEP_SECONDS);
        assert_eq!(g.clock.total_minutes(), t0 + 30.0);
        assert!(!g.waiting());
        // Left alone it goes on far past any wait of the buttons.
        g.begin_endless_wait();
        for _ in 0..40 {
            g.tick(STEP_SECONDS);
        }
        assert_eq!(g.clock.total_minutes(), t0 + 30.0 + 40.0 * 30.0);
        assert!(g.endless_waiting());
        // F5 after a tick: it stops at once, the tick under way not counted.
        g.tick(STEP_SECONDS * 0.5);
        g.end_endless_wait();
        assert!(!g.waiting() && !g.endless_waiting());
        g.tick(STEP_SECONDS * 2.0);
        assert_eq!(g.clock.total_minutes(), t0 + 30.0 + 40.0 * 30.0);
        // A wait of the buttons is not endless.
        g.begin_wait(1);
        assert!(!g.endless_waiting());
    }

    #[test]
    fn armies_move_only_on_the_minutes_banked_from_the_hero() {
        let mut s = strip();
        // A lord who wanders: patrol radius 8 around (12, 2).
        let mut lord = army(1, 12, 2, 1, &[troop(4, 0, 1)]);
        lord.patrols = 1;
        lord.patrol_radius = 8;
        s.armies = vec![lord];
        let mut g = start(&s);
        let at = g.world.armies[0].pos;
        g.tick(10.0);
        assert_eq!(g.world.armies[0].pos, at, "the hero stands still: so does the world");
        // One orthogonal grass step ahead of it, and no plan before its countdown runs out.
        let a = &mut g.world.armies[0];
        a.mind.countdown = 100;
        a.path = vec![(13, 2), (14, 2)];
        let mut events = Vec::new();
        g.pass_time(20.0, &mut events);
        assert_eq!(g.world.armies[0].pos, at, "20 minutes do not pay a 25-minute grass step");
        g.pass_time(10.0, &mut events);
        assert_ne!(g.world.armies[0].pos, at, "30 do");
        // The bank holds 200 minutes at most (an army too slow to take a step: 5 × 100).
        g.world.armies[0].speed = 100;
        g.pass_time(24.0 * 60.0, &mut events);
        assert_eq!(g.world.armies[0].budget, 200.0);
    }

    #[test]
    fn armies_walk_their_steps_within_the_heros_step() {
        let mut s = strip();
        let mut lord = army(1, 12, 2, 1, &[troop(4, 0, 1)]);
        lord.patrols = 1;
        lord.patrol_radius = 8;
        s.armies = vec![lord];
        let mut g = start(&s);
        let a = &mut g.world.armies[0];
        let (x0, y0) = a.pos;
        a.mind.countdown = 100;
        a.path = vec![(13, 2), (14, 2), (15, 2), (16, 2)];
        // 50 banked + 25 minutes: two grass steps (25 each) and a third left unpaid.
        a.budget = 50.0;
        let mut events = Vec::new();
        g.pass_time(25.0, &mut events);
        g.since_step = 0.0;
        let a = &g.world.armies[0];
        assert_eq!(a.pos, (x0 + 3.0, y0), "the bank pays all three steps");
        let at = |g: &mut Game, k: f32| {
            g.since_step = k * STEP_SECONDS;
            g.army_display_pos(&g.world.armies[0])
        };
        // 75 minutes banked: the first two steps take a third of the window each, the last
        // the rest; the figure moves steadily from cell to cell, never jumping.
        assert_eq!(at(&mut g, 0.0), (x0, y0));
        assert!((at(&mut g, 1.0 / 6.0).0 - (x0 + 0.5)).abs() < 1e-4);
        assert!((at(&mut g, 1.0 / 3.0).0 - (x0 + 1.0)).abs() < 1e-4);
        assert!((at(&mut g, 0.5).0 - (x0 + 1.5)).abs() < 1e-4);
        assert_eq!(at(&mut g, 1.0), (x0 + 3.0, y0));
        let mut last = x0;
        for i in 1..=30 {
            let x = at(&mut g, i as f32 / 30.0).0;
            assert!(x >= last && x - last < 0.2, "step {i}: {last} -> {x}");
            last = x;
        }
        // Moved by other means since: drawn where it is.
        g.world.armies[0].pos = (1.0, 1.0);
        assert_eq!(at(&mut g, 0.5), (1.0, 1.0));
    }

    /// The armies' walk frames run by game time (engine.md §7): frames 3–6, the next every
    /// 10 game minutes, smoothly inside a stretch and standing still while time does; the
    /// standing frame without a step to take.
    #[test]
    fn army_walk_frames_follow_the_game_time() {
        let mut s = strip();
        s.armies = vec![army(1, 12, 2, 1, &[troop(4, 0, 1)])];
        let mut g = start(&s);
        g.world.armies[0].path.clear();
        assert_eq!(g.army_walk_frame(&g.world.armies[0]), None, "no step to take: standing");
        g.world.armies[0].path = vec![(13, 2)];
        let t0 = g.clock.total_minutes();
        g.stretch_minutes = 30.0;
        g.clock.set_total_minutes(t0 + 30.0);
        let frame = |g: &mut Game, k: f32| {
            g.since_step = k * STEP_SECONDS;
            g.army_walk_frame(&g.world.armies[0]).unwrap()
        };
        // 30 minutes of a tick: three frames go by, one per 10 game minutes, in 3..=6.
        let seen: Vec<u32> = (0..30).map(|i| frame(&mut g, i as f32 / 30.0)).collect();
        assert!(seen.iter().all(|f| (3..=6).contains(f)), "{seen:?}");
        let changes = seen.windows(2).filter(|w| w[0] != w[1]).count();
        assert!((2..=3).contains(&changes), "{seen:?}");
        // Time stands: so does the frame, however long the screen shows it.
        let still = frame(&mut g, 1.0);
        g.since_step = 50.0;
        assert_eq!(g.army_walk_frame(&g.world.armies[0]), Some(still));
    }

    /// A step in place takes its time on the figure's walk too: it stands for it, so a later
    /// step plays at its own moment (0x4a399c plays the steps in turn).
    #[test]
    fn a_step_in_place_stands_for_its_time() {
        let mut s = strip();
        let mut lord = army(1, 12, 2, 1, &[troop(4, 0, 1)]);
        lord.patrols = 1;
        lord.patrol_radius = 8;
        s.armies = vec![lord];
        let mut g = start(&s);
        let a = &mut g.world.armies[0];
        let at = a.pos;
        a.mind.scripted = true;
        a.path.clear();
        a.budget = 25.0;
        let mut events = Vec::new();
        g.pass_time(25.0, &mut events);
        let a = &g.world.armies[0];
        assert!(!a.walk.minutes.is_empty(), "it stepped in place");
        assert!(a.walk.points.iter().all(|&p| p == at));
        assert_eq!(a.walk.points.len(), a.walk.minutes.len() + 1);
        g.since_step = 0.5 * STEP_SECONDS;
        assert_eq!(g.army_display_pos(&g.world.armies[0]), at);
    }

    #[test]
    fn the_expert_setting_gives_battles_the_improved_ai() {
        let mut g = quiet_game(HeroClass::Knight);
        g.foe = Some(Foe::Garrison(g.world.index_of("Bandit camp")));
        assert_eq!(g.start_battle().ai_level, 1, "easy: the original's normal AI");
        g.improved_ai = true;
        assert_eq!(g.start_battle().ai_level, 2, "expert: improved enemy AI in battle");
    }

    #[test]
    fn a_unit_dragged_onto_another_cell_moves_or_swaps() {
        use crate::rules::formation::Row;
        let mut g = quiet_game(HeroClass::Knight);
        let taken: Vec<_> = g.squad.iter().map(|u| u.slot).collect();
        let free = g.content.formation.free_slot(&taken, Row::Back).unwrap();
        g.squad.push(Unit::new(&g.content, unit(&g, "archer"), free));
        let (hero_at, archer_at) = (g.squad[0].slot, g.squad[1].slot);
        let empty = g.content.formation.slots().find(|s| *s != hero_at && *s != archer_at).unwrap();
        assert!(g.move_unit(1, empty));
        assert_eq!(g.squad[1].slot, empty, "to an empty cell");
        assert!(g.move_unit(1, hero_at));
        assert_eq!((g.squad[1].slot, g.squad[0].slot), (hero_at, empty), "onto the hero: they swap");
        assert!(!g.move_unit(1, crate::rules::formation::Slot::new(Row::Front, 99)), "not a cell");
    }

    #[test]
    fn the_shown_route_is_the_one_walked() {
        let mut g = start(&strip());
        g.fog = Fog::disabled(24, 6);
        let shown = g.route_to((20, 2));
        assert!(!shown.is_empty());
        assert!(!g.moving(), "showing a route does not set off");
        assert!(g.set_destination((20, 2)));
        assert_eq!(g.path, shown);
    }

    #[test]
    fn the_music_and_the_event_chord_draw_from_the_games_generator() {
        let mut g = quiet_game(HeroClass::Knight);
        let wait = g.take_music_wait().expect("the map start drew it");
        assert!((90_000..=122_767).contains(&wait), "{wait}");
        assert_eq!(g.take_music_wait(), None, "taken once");
        let mut r = g.rng.clone();
        let chord = g.event_chord();
        assert_eq!(chord, r.random(3) as usize);
        assert_eq!(g.rng.state(), r.state(), "one draw");
        let (pick, ms) = g.music_rotate(1);
        assert_eq!((pick, ms), crate::rules::music::rotate(1, &mut r));
        assert_eq!(g.rng.state(), r.state());
    }

    #[test]
    fn a_cut_walk_ends_with_the_step_under_way() {
        let mut g = start(&strip());
        g.fog = Fog::disabled(24, 6);
        let from = g.tile();
        assert!(g.set_destination((20, 2)));
        let first = g.path[0];
        assert!(g.path.len() > 2);
        g.tick(STEP_SECONDS * 0.5);
        g.cut_walk();
        assert_eq!(g.path, vec![first], "the step under way stays");
        assert_eq!(g.goal, Some((20, 2)), "only the route is cut");
        walk_until_stopped(&mut g);
        assert_ne!(g.tile(), from);
        assert_eq!(g.tile(), first, "he stops on the step's cell");
    }

    #[test]
    fn a_moving_army_is_walked_through_and_engaged_on_the_step() {
        // World.md §1.3 (0x4cc583): only stationary guards close the hero's route; a
        // patrolling army standing in the gap leaves it open. Stepping onto its cell engages
        // it (§4.2, 0x4ad94c): he stops on the cell before it.
        let mut s = strip();
        for y in [0u32, 1, 3, 4, 5] {
            tk::set(&mut s, 10, y, crate::dt::dtm::Surface::DeepSea);
        }
        let mut foe = army(1, 10, 2, -2, &[troop(4, 0, 1)]);
        foe.patrols = 1;
        foe.patrol_radius = 5;
        s.armies = vec![foe];
        let mut g = start(&s);
        g.fog = Fog::disabled(24, 6);
        let a = &mut g.world.armies[0];
        a.mind.scripted = true;
        a.path.clear();
        assert!(g.set_destination((20, 2)), "a patrolling army in the gap does not close it");
        assert!(g.path.contains(&(10, 2)));
        assert_eq!(g.talk_to, None, "the army is not the one clicked");
        let events = walk_until_stopped(&mut g);
        assert_eq!((events.last(), g.tile(), g.foe), (Some(&Event::Encounter(0)), (9, 2), Some(Foe::Army(0))));
        // Gone from the gap before he gets there, it is not met: he walks on.
        let mut g = start(&s);
        g.fog = Fog::disabled(24, 6);
        g.world.armies[0].mind.scripted = true;
        g.world.armies[0].path.clear();
        assert!(g.set_destination((20, 2)));
        g.world.armies[0].pos = g.world.map.center((12, 5));
        walk_until_stopped(&mut g);
        assert_eq!(g.tile(), (20, 2));
    }

    #[test]
    fn stationary_guards_block_the_route_except_the_one_clicked() {
        let mut s = strip();
        for y in [0u32, 1, 3, 4, 5] {
            tk::set(&mut s, 10, y, crate::dt::dtm::Surface::DeepSea);
        }
        let mut guard = army(1, 10, 2, 1, &[troop(4, 0, 1)]);
        guard.patrols = 1;
        guard.patrol_radius = 0;
        s.armies = vec![guard];
        let mut g = start(&s);
        g.fog = Fog::disabled(24, 6);
        assert!(!g.set_destination((20, 2)), "a guard standing in the gap");
        assert!(g.set_destination((10, 2)), "the guard himself can be clicked");
    }

    #[test]
    fn scenario_villages_pay_gold_and_mana_tribute() {
        let mut s = strip();
        let mut v = building(BuildingType::Village, 6, 2, (1, 1));
        v.gold_per_day = 25;
        v.gold_max = 60;
        v.mana_per_day = 4;
        v.mana_max = 10;
        v.relations = [1, 0, 0, 0];
        s.buildings = vec![v];
        let mut g = start(&s);
        g.set_destination((6, 2));
        // Entering takes it all at once (economy.md §3), unless the village asks something first.
        let events = walk_until_stopped(&mut g);
        if g.village_offer().is_some() {
            assert_eq!(g.decline_offer(), Some(Tribute::Gold(25)));
        } else {
            let arrived = events.iter().position(|e| e == &Event::Arrived(0)).expect("arrived");
            assert_eq!(events.get(arrived + 1), Some(&Event::Tribute { at: 0, paid: Tribute::Gold(25), mana: 4 }), "{events:?}");
        }
        assert_eq!(g.mana, 4);
        assert_eq!(g.tribute_available(), None);
        g.wait(24);
        assert_eq!(g.tribute_available(), Some(25), "refilled at midnight");
    }
}


