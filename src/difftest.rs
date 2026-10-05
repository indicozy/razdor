//! Script mode for the diff test (`tools/difftest/`): a game played without a window from a
//! list of actions, with the game state written after each one, so that the same actions
//! played in the original Discord Times can be compared with Razdor step by step.
//!
//! The actions (action list v1, one JSON object per line) go through the game's own rules
//! the way the interface applies them (`ui::world_view`, `ui::mod`): a map click walks to
//! the cell with the same steps and game time a second click on the shown route gives,
//! waits play their 30-minute ticks, scenario messages queue as dialogs that `ok` or
//! `answer` closes, a pending fight opens the battle once no dialog is open, and the
//! game's generator takes the same draws the interface makes (the chord of an event, village
//! or shipyard window as it opens; the music change when a dialog closes after a won
//! battle). What the interface does in real time (the map music's timed rotation) is left
//! out: it has no fixed place in a script.
//!
//! The state (schema v1) uses the original's encodings, so the two sides compare as they
//! are: see [`State`].

use std::collections::VecDeque;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::av::{self, AvEvent, AvLog, BattleSound};
use crate::dt::dtm::Scenario;
use crate::dt::install::DtInstall;
use crate::rules::battle::{Battle, Outcome, Step, Team};
use crate::rules::formation::{Row, Slot};
use crate::rules::content::{Content, HeroClass};
use crate::rules::events::EventOutcome;
use crate::rules::game::{BattleResult, Event, Foe, Game, STEP_SECONDS};
use crate::rules::script::ScriptEnd;
use crate::rules::town::Tab;
use crate::rules::world::{Army, LocationKind, Owner, Troop};

/// One action of the list (action list v1).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Action {
    /// A new game on map file `map` (its file name, with or without `.DTm`; `demo` for the
    /// built-in demo) with hero preset `hero`: 1 knight, 2 archmage, 3 ranger. Any map of the
    /// install starts, later campaign maps too; `carry` stands in for what a campaign carries
    /// over from the map before (Razdor has no carry-over yet; the original only starts a
    /// campaign at its first map).
    NewGame {
        map: String,
        hero: u8,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        carry: Option<Carry>,
    },
    /// A click on map cell (x, y): the hero walks there (both clicks of the original).
    ClickMap { x: i32, y: i32 },
    /// Wait 1 or 4 hours.
    Wait { hours: u32 },
    /// A key: `Escape` (closes a building window), `1` / `4` (wait), `Return` / `space` (ok).
    Key { key: String },
    /// Answers the question shown.
    Answer { yes: bool },
    /// Closes the dialog shown, else the building window.
    Ok,
    /// Plays the battle shown to its end by the battle AI on both sides and closes its
    /// result box.
    BattleAuto,
    /// In battle, a press on the card at `row`, `col` of `side` (1 the player's, 2 the
    /// enemy's; rows 1 front, 2 back, 3 reserve and columns 1–6 as the original's grid
    /// numbers them): the action that cell holds for the unit whose turn it is (a strike, a
    /// shot or a spell on an enemy, a heal or a blessing on a friend, a pass on its own card),
    /// or a step to an empty own cell. The enemy's turns then play until the player's next.
    BattleAct { side: u8, row: i32, col: i32 },
    /// In battle, the space key: what a press on the acting unit's own card does (a pass,
    /// or a self-cast when its cell holds one).
    BattlePass,
    /// Nothing: only the state is written.
    Snapshot,
    /// In the window of the building the hero stands in (its market tab): buys the good of
    /// row `slot` (0-based) of the goods list as the window shows it.
    Buy { slot: usize },
    /// The market tab's sell list (the pack's items worth more than 1, in pack order): sells
    /// the item of row `slot`.
    Sell { slot: usize },
    /// The hire tab: hires one unit of barracks slot `slot` (0-based, the building record's
    /// slot order).
    Hire { slot: usize },
    /// The hire tab: heals unit `unit` of the hero's army (0-based, record order) for its price.
    Heal { unit: usize },
    /// The hire tab of a town or church: raises the dead unit `unit` of the hero's army.
    Resurrect { unit: usize },
    /// The sanctuary tab: learns the spell of row `slot` of its list.
    Learn { slot: usize },
    /// The main hall tab: takes the quest or rumour of row `slot` (0-based) of its list
    /// (Take quest, 0x4bb798): its window opens at once.
    Take { slot: usize },
    /// On the map: casts the spell of book entry `slot` (0-based) on the hero's army, or, for
    /// a spell on an enemy, on the army with map id `army`. A building window is closed first.
    Cast {
        slot: usize,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        army: Option<i32>,
    },
    /// The army window: puts pack item `slot` (0-based in pack order, empty places left out)
    /// on unit `unit` of the hero's army. A building window is closed first.
    Equip { slot: usize, unit: usize },
}

/// A start on a later campaign map with what the map before carries over, given by hand in
/// the state's encodings (unit `[type, level]` with a 1-based type and a 0-based level, item
/// and spell numbers, the campaign flags by name). It goes through Razdor's own hand-over
/// ([`Game::from_campaign`], 0x4b5b64), so the map's opening events see the carried army,
/// gold and flags. A field left out is not carried (the map's own preset stays).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Carry {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gold: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mana: Option<i32>,
    /// The hero's level (0-based); with it his book (`book`) replaces the map's, as the
    /// hand-over's byte 3 does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hero_level: Option<i32>,
    /// The army after the hero (replaces the map's preset troops).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub units: Vec<[i32; 2]>,
    /// The named character of each of `units` (0 none), as an event of an earlier map
    /// added it (an event's unit: no wage).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub named: Vec<u8>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pack: Vec<i32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub book: Vec<i32>,
    /// The campaign flags set on the maps before (event title scripts `%+X`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flags: Vec<String>,
    /// The whole map explored (a player who knows it from the campaign's earlier play), so
    /// that a click plans the route the planner finds over all of it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub reveal: bool,
}

impl Carry {
    /// The hand-over this stands for, onto map `map` for class `class`.
    fn next_map(&self, c: &Content, map: &str, class: HeroClass) -> crate::rules::script::NextMap {
        use crate::rules::content::{ItemId, UnitId};
        use crate::rules::units::Unit;
        let front = Slot::new(Row::Front, c.formation.cols / 2);
        let mut hero = Unit::new(c, class.unit(), front);
        hero.level = self.hero_level.map_or(1, |l| l + 1);
        hero.wage_kind = crate::rules::content::WageKind::Leader;
        hero.heal_full(c);
        let mut taken = vec![front];
        let mut army = Vec::new();
        for (i, &[kind, level]) in self.units.iter().enumerate() {
            let Some(slot) = c.formation.new_unit_slot(&taken) else { break };
            taken.push(slot);
            let mut u = Unit::new(c, UnitId(kind as u32), slot);
            u.level = level + 1;
            u.heal_full(c);
            u.named = self.named.get(i).copied().unwrap_or(0);
            if u.named != 0 {
                u.from_event = true;
                u.wage_kind = crate::rules::content::WageKind::Event;
            }
            army.push(u);
        }
        crate::rules::script::NextMap {
            name: map.to_string(),
            branch: None,
            gold: self.gold,
            mana: self.mana,
            fame: false,
            hero,
            spells: self.hero_level.map(|_| self.book.iter().map(|&s| s as u8).collect()),
            hero_items: true,
            inventory: (!self.pack.is_empty()).then(|| self.pack.iter().map(|&i| ItemId(i as u32)).collect()),
            army: (!self.units.is_empty()).then_some(army),
            flags: self.flags.iter().map(|f| format!("{f}\u{a0}")).collect(),
            class,
            hero_name: None,
            journal: Default::default(),
        }
    }

    fn reveal_all(&self, g: &mut Game) {
        if self.reveal {
            let (w, h) = (g.world.map.w, g.world.map.h);
            for y in 0..h {
                for x in 0..w {
                    g.fog.mark((x, y));
                }
            }
        }
    }
}

/// A unit of the hero's army.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HeroUnit {
    #[serde(rename = "type")]
    pub kind: i32,
    pub level: i32,
    pub hp: i32,
    pub xp: i32,
    /// The four worn item slots (item `GlobalIndex`, 0 empty), unit +0xcd.
    #[serde(default)]
    pub items: Vec<i32>,
}

/// A unit of an AI army.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ArmyUnit {
    #[serde(rename = "type")]
    pub kind: i32,
    pub level: i32,
    pub hp: i32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HeroState {
    pub x: i32,
    pub y: i32,
    pub gold: i32,
    pub mana: i32,
    pub units: Vec<HeroUnit>,
    /// The pack's items (`GlobalIndex`) in pack order, empty places left out (0x68dce0).
    #[serde(default)]
    pub pack: Vec<i32>,
    /// The spell book (1-based spell numbers) in book order (0x68e0e8).
    #[serde(default)]
    pub book: Vec<i32>,
}

/// An army record. Fields Razdor no longer has for an army it dropped are left out.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ArmyState {
    pub id: i32,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub x: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub y: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub active: Option<bool>,
    pub alive: bool,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub gold: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub units: Option<Vec<ArmyUnit>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BuildingState {
    pub id: i32,
    pub owner: i32,
    pub gold: i32,
    pub mana: i32,
    pub goods: Vec<i32>,
}

/// The game state after a step (state schema v1), with the meanings the original's side reads
/// (`tools/difftest/memread.py`):
/// - `clock`: the calendar minute, `[0x68dcb8] div 100 + [0x68dcbc]` (world.md: game time in
///   centi-minutes since the map's start plus the start minute, the map file's start minute
///   + 1);
/// - `rng`: the state of the game's generator (engine.md §3.1);
/// - hero `x`, `y`: his cell; `gold`, `mana`: the player's;
/// - unit `type`: the map file's unit number (1-based; the record's +0 is 0-based); `level`:
///   0 for the first, as the map file and the unit record (+0x10) number it; `hp`: the
///   hit points it has (the record stores −1 for unhurt, read as its maximum), 0 dead;
///   units in record order, corpses included;
/// - army `id`: its number in the map file; `active`: on the map (+0x16a1); `alive`: not
///   destroyed (+0x16a2); `gold` (+0x16d8);
/// - building `owner` (+0x124): 0 the player, k army k, 255 none; `gold` / `mana` its
///   stocks (+0x11e, +0x160); `goods`: the items of its goods words, without the sign (a
///   negative one is the map's own) and the empty ones, in place order;
/// - `events_done`: the events that have fired at least once (event +0xa0).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct State {
    pub step: usize,
    pub map: String,
    pub clock: u64,
    pub rng: u32,
    pub hero: HeroState,
    pub armies: Vec<ArmyState>,
    pub buildings: Vec<BuildingState>,
    pub events_done: Vec<i32>,
    /// The battle on screen, if any.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub battle: Option<BattleState>,
}

/// The battle on screen (schema v1's `battle`), with the original's encodings (the battle
/// object at 0x668cf8, battle.md): `turn` the battle turn from 1 (+0xd); `actor` the unit
/// whose turn it is as `[side, row, col]` (side 1 the player's, 2 the enemy's), none once it
/// is over; `sides[0]` the player's units, `sides[1]` the enemy's, in their record order
/// (a dead unit's record is removed, the later ones move up).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BattleState {
    pub turn: u32,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub actor: Option<[i32; 3]>,
    pub sides: [Vec<BattleUnit>; 2],
}

/// A unit's record in battle: `type` 1-based (+0x23), `row` 1–3 and `col` 1–6 (+0x75,
/// +0x79), `hp` its hit points (+0x7d), `actions` the actions it has left this turn (+0x91).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BattleUnit {
    #[serde(rename = "type")]
    pub kind: i32,
    pub row: i32,
    pub col: i32,
    pub hp: i32,
    pub actions: i32,
}

/// What is on screen.
enum Screen {
    Map,
    /// The window of the building the party stands in.
    Building,
    Battle(Box<Battle>),
    /// The scenario is won or lost, or the army fell.
    Ended,
}

/// A dialog waiting to be read: an event's, the victory box or the noon report, all in the
/// original's event window, which draws a chord as it opens (0x4d15d0).
struct Dialog {
    event: bool,
    question: bool,
    cued: bool,
    /// A village's offer: the original asks it in the event window (slot N + 2, 0x4aca80).
    offer: bool,
    /// The map event whose window it is (1-based), if it is one.
    id: Option<u16>,
}

impl Dialog {
    fn message() -> Self {
        Dialog { event: true, question: false, cued: false, offer: false, id: None }
    }
}

/// Where the game's content comes from.
pub enum Source<'a> {
    Demo,
    Install(&'a DtInstall),
}

/// A game played from actions.
pub struct Runner<'a> {
    source: Source<'a>,
    content: Option<Arc<Content>>,
    game: Option<Game>,
    scenario: Option<Scenario>,
    map: String,
    screen: Screen,
    last_screen_building: bool,
    dialogs: VecDeque<Dialog>,
    /// A won battle's triumph plays: closing a dialog changes the map track at once (a draw).
    triumph: bool,
    music_pick: usize,
    /// The generator draws the interface made (window chords, music changes).
    pub ui_draws: usize,
    /// What could not be applied, one line each.
    pub notes: Vec<String>,
    /// The sounds, tracks and animations the interface would play (`crate::av`), at the
    /// points where `ui` cues them.
    pub av: AvLog,
    /// The building window's tab, while one is open (`ui::building_view`).
    tab: Option<Tab>,
    /// Its market shows the sell list (the pack) rather than the goods.
    selling: bool,
    /// The open village window took the tribute: its close plays `Item-Gold` (0x4c604a).
    tribute_due: bool,
    /// Places an event showed, with that event: flown to once its window is closed
    /// (`world_view`).
    shows: Vec<(Option<u16>, crate::rules::map::Tile)>,
}

/// Safety stop for a walk or a wait that does not end.
const MAX_TICKS: usize = 200_000;

impl<'a> Runner<'a> {
    pub fn new(source: Source<'a>) -> Self {
        Runner {
            source,
            content: None,
            game: None,
            scenario: None,
            map: String::new(),
            screen: Screen::Map,
            last_screen_building: false,
            dialogs: VecDeque::new(),
            triumph: false,
            music_pick: crate::rules::music::WORLD_THEME,
            ui_draws: 0,
            notes: Vec::new(),
            av: AvLog::default(),
            tab: None,
            selling: false,
            tribute_due: false,
            shows: Vec::new(),
        }
    }

    pub fn game(&self) -> Option<&Game> {
        self.game.as_ref()
    }

    /// What the replay left on screen, for a picture of it (`RAZDOR_SCENE=replay`): the game,
    /// the battle if one is open, and whether a building window is open. The dialogs waiting
    /// to be read are not handed over.
    pub fn into_view(self) -> Option<(Game, Option<Box<Battle>>, bool)> {
        let game = self.game?;
        Some(match self.screen {
            Screen::Battle(b) => (game, Some(b), false),
            Screen::Building => (game, None, true),
            Screen::Map | Screen::Ended => (game, None, false),
        })
    }

    /// Facts outside the schema (for the run log): the battle units' current stats, as the
    /// original's records hold them, and the building defence of each side.
    pub fn raw(&self) -> serde_json::Value {
        use crate::dt::data::Stat;
        let Screen::Battle(b) = &self.screen else { return serde_json::Value::Null };
        let side = |team: Team| -> Vec<serde_json::Value> {
            b.fighters
                .iter()
                .filter(|f| f.team == team && f.listed())
                .map(|f| {
                    let st = &f.stats;
                    serde_json::json!({"ab": st[Stat::AttackBlow], "as": st[Stat::AttackShot], "mp": st[Stat::MagicPower],
                        "db": st[Stat::DefenceBlow], "ds": st[Stat::DefenceShot], "maxhp": f.max_hp(), "manevres": st[Stat::Manevres],
                        "init": st[Stat::Initiative], "atk_mod": f.mods.attack, "def_mod": f.mods.defence, "init_mod": f.mods.initiative,
                        "bld_def": b.building_defence(team)})
                })
                .collect()
        };
        serde_json::json!({"battle_raw": {"sides": [side(Team::Player), side(Team::Enemy)]}})
    }

    /// Sets the game's generator (the diff test's step-local mode: each step starts from the
    /// original's state of the step before).
    pub fn set_rng(&mut self, state: u32) {
        if let Some(g) = self.game.as_mut() {
            g.rng = crate::rules::rng::Rng::new(state);
        }
    }

    fn note(&mut self, s: String) {
        self.notes.push(s);
    }

    /// Applies one action.
    pub fn apply(&mut self, action: &Action) -> Result<(), String> {
        if let Action::NewGame { map, hero, carry } = action {
            return self.new_game(map, *hero, carry.as_ref());
        }
        if self.game.is_none() {
            return Err("no game: the list must start with new_game".into());
        }
        match action {
            Action::NewGame { .. } => unreachable!(),
            Action::ClickMap { x, y } => self.click((*x, *y)),
            Action::Wait { hours } => self.wait(*hours),
            Action::Key { key } => match key.as_str() {
                "Escape" | "Esc" => self.close_building(),
                "1" => self.wait(1),
                "4" => self.wait(4),
                "Return" | "Enter" | "space" | "Space" => self.ok(),
                other => self.note(format!("key {other}: not applied")),
            },
            Action::Answer { yes } => self.answer(*yes),
            Action::Ok => self.ok(),
            Action::BattleAuto => self.battle_auto(),
            Action::BattleAct { side, row, col } => self.battle_act(*side, *row, *col),
            Action::BattlePass => self.battle_pass(),
            Action::Snapshot => {}
            Action::Buy { slot } => self.buy(*slot),
            Action::Sell { slot } => self.sell(*slot),
            Action::Hire { slot } => self.hire(*slot),
            Action::Heal { unit } => self.heal(*unit, false),
            Action::Resurrect { unit } => self.heal(*unit, true),
            Action::Learn { slot } => self.learn(*slot),
            Action::Take { slot } => self.take(*slot),
            Action::Cast { slot, army } => self.cast(*slot, *army),
            Action::Equip { slot, unit } => self.equip(*slot, *unit),
        }
        self.settle();
        Ok(())
    }

    fn new_game(&mut self, map: &str, hero: u8, carry: Option<&Carry>) -> Result<(), String> {
        let class = match hero {
            1 => HeroClass::Knight,
            2 => HeroClass::Archmage,
            3 => HeroClass::Ranger,
            n => return Err(format!("hero {n}: 1, 2 or 3")),
        };
        let stem = map.trim().trim_end_matches(".DTm").trim_end_matches(".dtm");
        let mut game = match (&self.source, stem) {
            (_, "demo") | (Source::Demo, _) => {
                if stem != "demo" {
                    return Err(format!("map {map}: no install, only the demo"));
                }
                self.scenario = None;
                self.map = "demo".into();
                Game::new(Arc::new(Content::builtin()), class)
            }
            (Source::Install(dt), _) => {
                let exact = dt.maps.iter().find(|m| m.name == stem);
                let found = exact.or_else(|| {
                    let mut it = dt.maps.iter().filter(|m| m.name.starts_with(stem));
                    let first = it.next();
                    first.filter(|_| it.next().is_none())
                });
                let m = found.ok_or_else(|| format!("map {map}: not in the install (or not one map)"))?;
                let s = m.load().map_err(|e| format!("map {map}: {e}"))?;
                let content = match &self.content {
                    Some(c) => c.clone(),
                    None => {
                        let c = Arc::new(Content::from_dt(dt));
                        self.content = Some(c.clone());
                        c
                    }
                };
                let mut g = match carry {
                    Some(k) => {
                        let mut g = Game::from_campaign(content.clone(), &s, &k.next_map(&content, &m.name, class));
                        k.reveal_all(&mut g);
                        g
                    }
                    None => Game::from_scenario(content, &s, class),
                };
                g.improved_ai = dt.settings.expert_ai;
                self.map = format!("{}.DTm", m.name);
                self.scenario = Some(s);
                g
            }
        };
        game.set_hero_name("");
        // The way there with the mouse (`main_menu`, `new_game`): the bell as the pointer
        // comes onto New game and its press, the scenario's row (silent), Next, the class
        // portrait when it changes the class (the knight is picked to begin with), Start.
        self.av.sfx("MainMenuSelect-1");
        self.av.sfx("MainMenuPress");
        self.av.sfx("InterfaceButtonDown");
        if class != HeroClass::Knight {
            self.av.sfx("MainMenuPress");
        }
        self.av.sfx("InterfaceButtonDown");
        // The fog opens around the hero (0x4af83c).
        let at = game.tile();
        self.av.anim("reveal", Some(format!("{},{}", at.0, at.1)));
        self.game = Some(game);
        self.screen = Screen::Map;
        self.last_screen_building = false;
        self.dialogs.clear();
        self.triumph = false;
        self.music_pick = crate::rules::music::WORLD_THEME;
        self.ui_draws = 0;
        self.tab = None;
        self.tribute_due = false;
        self.shows.clear();
        self.settle();
        Ok(())
    }

    fn g(&mut self) -> &mut Game {
        self.game.as_mut().expect("a game")
    }

    /// The world map takes input: no dialog, no other screen.
    fn map_idle(&self) -> bool {
        self.dialogs.is_empty() && matches!(self.screen, Screen::Map)
    }

    /// Closes the building window (Esc, or the harness's `ok`): the building window's close
    /// is silent; the village and shipyard windows close with the button sound, and a
    /// village whose tribute was taken plays `Item-Gold` (0x4c604a).
    fn close_building(&mut self) {
        if matches!(self.screen, Screen::Building) && self.dialogs.is_empty() {
            if self.chord_window() {
                self.av.sfx("InterfaceButtonDown");
                if std::mem::take(&mut self.tribute_due) {
                    self.av.sfx("Item-Gold");
                }
            }
            self.screen = Screen::Map;
            // Closed now: opening it again (a click on the building he stands in) is a new
            // window, with its chord.
            self.last_screen_building = false;
            self.tab = None;
            // After a heal, a raise or a trade the events are checked as it closes (0x4b8f63).
            let events = self.g().window_closed();
            self.handle(events);
        }
    }

    /// The building he stands in opens the village or shipyard window (with a chord), not
    /// the building window.
    fn chord_window(&self) -> bool {
        let g = self.game.as_ref().expect("a game");
        g.location.is_some_and(|l| matches!(g.world.locations[l].kind, LocationKind::Village | LocationKind::Shipyard))
    }

    /// Switches the building window to `want` as the harness does (`original.py open_tab`):
    /// the tab buttons pressed from the top until it shows; each press on another tab than
    /// the one shown plays `InterfaceCastSpell` (interface.md §9.8).
    fn open_tab(&mut self, want: Tab) {
        let tabs = self.g().tabs_here();
        if !tabs.contains(&want) {
            return;
        }
        for t in tabs {
            if self.tab == Some(want) {
                break;
            }
            if self.tab != Some(t) {
                self.av.sfx("InterfaceCastSpell");
                self.tab = Some(t);
                if t == Tab::Market {
                    // The market opens on the goods when it has some.
                    self.selling = self.g().market_here().is_none_or(|g| g.is_empty());
                }
            }
        }
    }

    /// The market's list switch, pressed when the other list is shown.
    fn market_list(&mut self, sell: bool) {
        if self.selling != sell {
            self.av.sfx("InterfaceButtonDown");
            self.selling = sell;
        }
    }

    fn click(&mut self, t: (i32, i32)) {
        // A building window open: it is closed first (its Exit, as before any map click).
        self.close_building();
        if !self.map_idle() {
            self.note(format!("click_map {t:?}: the map takes no input now"));
            return;
        }
        let g = self.g();
        // A click on the building the party stands in opens it again (or its fight).
        if let Some(l) = g.location {
            let w = &g.world;
            if w.location_covering(t).or_else(|| w.location_at(t)) == Some(l) {
                let loc = &w.locations[l];
                if loc.kind == LocationKind::Camp && loc.cleared {
                    return;
                }
                if loc.defended() {
                    g.foe = Some(crate::rules::game::Foe::Garrison(l));
                } else if g.window_at(l).is_some() {
                    self.screen = Screen::Building;
                }
                return;
            }
        }
        if !g.can_target(t) || t == g.tile() {
            self.note(format!("click_map {t:?}: not a target"));
            return;
        }
        if g.set_destination(t) {
            self.av.anim("walk", Some(format!("{},{}", t.0, t.1)));
        } else {
            self.note(format!("click_map {t:?}: no way there"));
        }
    }

    fn wait(&mut self, hours: u32) {
        self.close_building();
        if !self.map_idle() || self.g().foe.is_some() {
            self.note(format!("wait {hours}: the map takes no input now"));
            return;
        }
        // The wait button's press (`world_view`: the keys and the time panel).
        self.av.sfx("InterfaceButtonDown");
        self.g().begin_wait(hours);
        self.av.anim("wait", Some(hours.to_string()));
    }

    /// Closes the front dialog; the music changes if the triumph plays.
    fn close_dialog(&mut self) {
        // Its OK, Yes or No button (`widgets::button`).
        self.av.sfx("InterfaceButtonDown");
        let closed = self.dialogs.pop_front();
        if self.triumph {
            let g = self.game.as_mut().expect("a game");
            let (pick, _) = g.music_rotate(self.music_pick);
            self.music_pick = pick;
            self.av.music(crate::rules::music::ROTATION[pick]);
            self.triumph = false;
            self.ui_draws += 1;
        }
        // A scenario event's window: the events after it run now.
        if closed.is_some_and(|d| d.id.is_some() && !d.question) {
            let events = self.g().event_window_closed();
            self.handle(events);
        }
    }

    /// The windows are read: the building he walked into while one opened is entered now
    /// (0x4bbc84).
    fn enter_waiting(&mut self) {
        if self.dialogs.is_empty() {
            let events = self.g().enter_waiting_building();
            self.handle(events);
        }
    }

    fn ok(&mut self) {
        match self.dialogs.front() {
            Some(d) if d.question => self.note("ok: a question is shown (answer it)".into()),
            Some(_) => {
                self.close_dialog();
                self.enter_waiting();
            }
            None if matches!(self.screen, Screen::Building) => self.close_building(),
            None => self.note("ok: nothing to close".into()),
        }
    }

    fn answer(&mut self, yes: bool) {
        if !self.dialogs.front().is_some_and(|d| d.question) {
            self.note("answer: no question shown".into());
            return;
        }
        if self.dialogs.front().is_some_and(|d| d.offer) {
            self.close_dialog();
            let g = self.g();
            if yes {
                // Yes: the offer's results, the village emptied (0x4ab966); the furs, the
                // witch and the innkeeper show their result in the event window again first
                // (its chord; OK closes it).
                let window = g.village_offer().is_some_and(|o| o.result_window());
                g.accept_offer();
                if window {
                    self.dialogs.push_back(Dialog::message());
                }
            } else {
                // No: the village is entered again with no offer (0x4c2378): its window,
                // the tribute taken.
                if g.decline_offer().is_some() {
                    self.tribute_due = true;
                }
                self.screen = Screen::Building;
            }
            self.enter_waiting();
            return;
        }
        self.close_dialog();
        let events = self.g().answer_question(yes);
        self.handle(events);
        self.enter_waiting();
    }

    /// The building window is open and takes input (no dialog over it).
    fn in_building(&mut self, op: &str) -> bool {
        if matches!(self.screen, Screen::Building) && self.dialogs.is_empty() {
            return true;
        }
        self.note(format!("{op}: no building window open"));
        false
    }

    fn buy(&mut self, slot: usize) {
        if !self.in_building("buy") {
            return;
        }
        self.open_tab(Tab::Market);
        self.market_list(false);
        match self.g().buy(slot) {
            // The trade button (`building_view::market`).
            Ok(_) => self.av.sfx("Item-Gold"),
            Err(e) => self.note(format!("buy {slot}: {e:?}")),
        }
    }

    /// The pack index of row `slot` of the market's sell list (items worth more than 1).
    fn sell_row(g: &Game, slot: usize) -> Option<usize> {
        (0..g.pack.len()).filter(|&k| g.can_sell(g.pack[k])).nth(slot)
    }

    fn sell(&mut self, slot: usize) {
        if !self.in_building("sell") {
            return;
        }
        self.open_tab(Tab::Market);
        self.market_list(true);
        let g = self.g();
        let r = match Self::sell_row(g, slot) {
            Some(k) => g.sell(k).map(|_| ()).map_err(|e| format!("{e:?}")),
            None => Err("no such row".into()),
        };
        match r {
            Ok(()) => self.av.sfx("Item-Gold"),
            Err(e) => self.note(format!("sell {slot}: {e}")),
        }
    }

    fn hire(&mut self, slot: usize) {
        if !self.in_building("hire") {
            return;
        }
        self.open_tab(Tab::Barracks);
        let g = self.g();
        let kind = g.location.and_then(|l| g.world.locations[l].recruits.iter().find(|r| r.slot as usize == slot).map(|r| r.unit));
        let r = match kind {
            Some(k) => g.hire(k).map_err(|e| format!("{e:?}")),
            None => Err("no such barracks slot".into()),
        };
        match r {
            // The hire pill (`building_view::barracks`): the gold sound of its two call
            // sites, then the new card slides into the army with `Card-Move` (0x4b0c04).
            Ok(()) => {
                self.av.sfx("Item-Gold");
                self.av.sfx("Item-Gold");
                self.av.anim("army_slot_slide", None);
                self.av.sfx("Card-Move");
            }
            Err(e) => self.note(format!("hire {slot}: {e}")),
        }
    }

    fn heal(&mut self, unit: usize, raise: bool) {
        let op = if raise { "resurrect" } else { "heal" };
        if !self.in_building(op) {
            return;
        }
        self.open_tab(Tab::Barracks);
        let g = self.g();
        let r = if raise { g.resurrect(unit) } else { g.heal(unit) };
        match r {
            Ok(events) => {
                // The gold sound of the button, then the cure on the card (0x4b11cc).
                self.av.sfx("Item-Gold");
                self.av.anim("unit_action", None);
                self.av.sfx("Battle-Cure");
                self.handle(events);
            }
            Err(e) => self.note(format!("{op} {unit}: {e:?}")),
        }
    }

    fn learn(&mut self, slot: usize) {
        if !self.in_building("learn") {
            return;
        }
        self.open_tab(Tab::Sanctuary);
        let g = self.g();
        let id = g.spells_here().get(slot).map(|s| s.id);
        let r = match id {
            Some(id) => g.learn_spell(id).map_err(|e| format!("{e:?}")),
            None => Err("no such row".into()),
        };
        match r {
            Ok(()) => self.av.sfx("Item-Gold"),
            Err(e) => self.note(format!("learn {slot}: {e}")),
        }
    }

    /// The main hall's Take quest (0x4bb798) on row `slot`: the row's click is silent, the
    /// button sounds; the event's window opens over the building window.
    fn take(&mut self, slot: usize) {
        if !self.in_building("take") {
            return;
        }
        self.open_tab(Tab::MainHall);
        let g = self.g();
        let r = match g.hall_here().get(slot).copied() {
            Some(id) => g.take_hall_entry(id).map_err(|e| format!("{e:?}")),
            None => Err("no such row".into()),
        };
        match r {
            Ok(events) => {
                self.av.sfx("InterfaceButtonDown");
                self.handle(events);
            }
            Err(e) => self.note(format!("take {slot}: {e}")),
        }
    }

    /// The map takes a window's input: a building window is closed first (opening a side
    /// window closes the open one, interface.md §9).
    fn map_window(&mut self, op: &str) -> bool {
        self.close_building();
        if self.map_idle() && self.game.as_ref().is_some_and(|g| g.foe.is_none()) {
            // The bar's panel icon (`game_bar`); the window opens silent.
            self.av.sfx("InterfacePanelDown");
            return true;
        }
        self.note(format!("{op}: the map takes no input now"));
        false
    }

    fn cast(&mut self, slot: usize, army: Option<i32>) {
        use crate::rules::magic::{targets_enemy, CastTarget};
        if !self.map_window("cast") {
            return;
        }
        let g = self.g();
        let Some(spell) = g.spells.get(slot).and_then(|&id| g.spell(id as u32)).cloned() else {
            self.note(format!("cast {slot}: no such book entry"));
            return;
        };
        let target = if targets_enemy(&spell) {
            match army.and_then(|id| g.world.armies.iter().find(|a| a.id as i32 == id)) {
                Some(a) => CastTarget::Army(a.uid),
                None => {
                    self.note(format!("cast {slot}: the spell needs a target army on the map"));
                    return;
                }
            }
        } else {
            CastTarget::Own
        };
        match g.begin_cast(spell.id, target) {
            Ok(outcome) => {
                self.av.sfx("InterfaceCastSpell");
                // The camera goes to the target army (the hero's own for a spell on it) before
                // the reading, or after it for an enemy (0x4c2e34, 0x4cc148: 0x4afa98).
                let own = matches!(target, CastTarget::Own);
                if own {
                    self.av.anim("look_at_army", None);
                }
                if outcome.is_none() {
                    // The hero reads on the map, the clock running as in a wait.
                    self.av.anim("wait", Some("cast".into()));
                }
                if !own {
                    self.av.anim("look_at_army", None);
                }
                // A spell that lands at once (no reading) plays its landing now.
                if let Some(o) = outcome {
                    log_landing(&mut self.av, target, o);
                }
            }
            Err(e) => self.note(format!("cast {slot}: {e:?}")),
        }
    }

    fn equip(&mut self, slot: usize, unit: usize) {
        if !self.map_window("equip") {
            return;
        }
        let kind = self.game.as_ref().and_then(|g| g.pack.get(slot).map(|&i| g.content.item(i).kind));
        self.g().army_window_opened();
        match self.g().equip(unit, slot) {
            // Its sound as it is taken from the pack, and again as it is worn.
            Ok(()) => {
                if let Some(kind) = kind {
                    self.av.sfx(av::item_sound(kind));
                    self.av.sfx(av::item_sound(kind));
                }
            }
            Err(e) => self.note(format!("equip {slot} on {unit}: {e:?}")),
        }
    }

    /// What the screen shows, for the explorer (`--look`): the screen, the open dialog, and
    /// in a building window what its tabs offer, with the row numbers the actions take.
    pub fn look(&self) -> serde_json::Value {
        use serde_json::json;
        let Some(g) = self.game.as_ref() else { return serde_json::Value::Null };
        let c = &g.content;
        let screen = match (&self.screen, self.dialogs.front()) {
            (Screen::Ended, _) => "ended",
            (_, Some(d)) if d.offer => "offer",
            (_, Some(d)) if d.question => "question",
            (_, Some(_)) => "dialog",
            (Screen::Battle(_), None) => "battle",
            (Screen::Building, None) => "building",
            (Screen::Map, None) => "map",
        };
        let mut out = json!({"screen": screen, "income": g.daily_income(), "wages": g.daily_wages()});
        if let Some(id) = self.dialogs.front().and_then(|d| d.id) {
            out["event"] = json!(id);
        }
        let book: Vec<_> = g
            .spells
            .iter()
            .enumerate()
            .filter_map(|(k, &id)| g.spell(id as u32).map(|s| (k, s)))
            .map(|(k, s)| json!({"slot": k, "id": s.id, "name": s.name, "mana": g.cast_cost(s).mana, "enemy": crate::rules::magic::targets_enemy(s)}))
            .collect();
        out["book"] = json!(book);
        let pack: Vec<_> = g.pack.iter().enumerate().map(|(k, &i)| json!({"slot": k, "id": i.0, "name": c.try_item(i).map_or("", |d| d.name.as_str())})).collect();
        out["pack"] = json!(pack);
        if let Some(o) = g.village_offer() {
            out["offer"] = json!(format!("{o:?}"));
        }
        if screen == "building" {
            if let Some(l) = g.location {
                let loc = &g.world.locations[l];
                out["building"] = json!({"id": loc.id, "kind": format!("{:?}", loc.kind)});
                out["tabs"] = json!(g.tabs_here().iter().map(|t| format!("{t:?}")).collect::<Vec<_>>());
                if let Some(goods) = g.market_here() {
                    out["goods"] = json!(goods.iter().enumerate().map(|(k, &i)| json!({"slot": k, "id": i.0, "name": c.try_item(i).map_or("", |d| d.name.as_str()), "price": g.buy_price(i)})).collect::<Vec<_>>());
                    let sell: Vec<_> = (0..g.pack.len()).filter(|&k| g.can_sell(g.pack[k])).enumerate().map(|(row, k)| json!({"slot": row, "id": g.pack[k].0, "price": g.sell_price(g.pack[k])})).collect();
                    out["sell"] = json!(sell);
                }
                if g.heals_here() {
                    out["recruits"] = json!(loc.recruits.iter().filter(|r| r.stock != Some(0)).map(|r| json!({"slot": r.slot, "type": r.unit.0, "name": c.unit(r.unit).name, "price": g.hire_price(r.unit).amount})).collect::<Vec<_>>());
                    let mut heal = Vec::new();
                    for i in 0..g.squad.len() {
                        if let Some(p) = g.heal_price(i) {
                            heal.push(json!({"unit": i, "op": "heal", "price": p.amount}));
                        } else if let Some(p) = g.resurrect_price(i).filter(|_| g.resurrects_here()) {
                            heal.push(json!({"unit": i, "op": "resurrect", "price": p.amount}));
                        }
                    }
                    out["services"] = json!(heal);
                }
                let hall: Vec<_> = g.hall_here().iter().enumerate().map(|(k, &id)| json!({"slot": k, "event": id})).collect();
                if !hall.is_empty() {
                    out["hall"] = json!(hall);
                }
                let spells: Vec<_> = g.spells_here().iter().enumerate().map(|(k, s)| json!({"slot": k, "id": s.id, "name": s.name, "price": s.cost_gold, "known": g.knows_spell(s.id)})).collect();
                if !spells.is_empty() {
                    out["spells"] = json!(spells);
                }
            }
        }
        out
    }

    fn battle_auto(&mut self) {
        let Screen::Battle(mut b) = std::mem::replace(&mut self.screen, Screen::Map) else {
            self.note("battle_auto: no battle".into());
            return;
        };
        b.auto_play_to_end();
        self.finish_battle(&b);
    }

    /// A press on a battle card (`Action::BattleAct`), as the battle window takes it.
    fn battle_act(&mut self, side: u8, row: i32, col: i32) {
        if let Err(e) = self.press_card(side, row, col) {
            self.note(format!("battle_act {side} {row} {col}: {e}"));
        }
        self.battle_play_ai();
    }

    fn press_card(&mut self, side: u8, row: i32, col: i32) -> Result<(), &'static str> {
        let Screen::Battle(b) = &mut self.screen else { return Err("no battle") };
        let active = b.active().filter(|&a| b.fighters[a].team == Team::Player).ok_or("not the player's turn")?;
        let team = match side {
            1 => Team::Player,
            2 => Team::Enemy,
            _ => return Err("side is 1 or 2"),
        };
        let row = match row {
            1 => Row::Front,
            2 => Row::Back,
            3 => Row::Reserve,
            _ => return Err("row is 1 to 3"),
        };
        let slot = Slot::new(row, u8::try_from(col - 1).map_err(|_| "col is 1 to 6")?);
        let done = match b.at(team, slot) {
            Some(t) => match b.options(active, t).first() {
                Some(&kind) => match b.act_with(t, kind) {
                    Ok(hit) => {
                        log_hit(&mut self.av, b, active, &hit);
                        true
                    }
                    Err(_) => false,
                },
                None if t == active => {
                    b.pass();
                    self.av.anim("battle_pass", None);
                    true
                }
                None => false,
            },
            None if team == Team::Player => {
                let moved = b.move_active(slot).is_ok();
                if moved {
                    log_move(&mut self.av, b, active);
                }
                moved
            }
            None => false,
        };
        if done {
            Ok(())
        } else {
            Err("no action on that card")
        }
    }

    /// The space key in battle: the acting unit's own-card action.
    fn battle_pass(&mut self) {
        let ok = match &mut self.screen {
            Screen::Battle(b) if b.active().is_some_and(|a| b.fighters[a].team == Team::Player) => {
                let actor = b.active().expect("an actor");
                match b.own_cell() {
                    Some(hit) => log_hit(&mut self.av, b, actor, &hit),
                    None => self.av.anim("battle_pass", None),
                }
                true
            }
            _ => false,
        };
        if !ok {
            self.note("battle_pass: not the player's turn in a battle".into());
        }
        self.battle_play_ai();
    }

    /// Plays the enemy's turns until the player's next one, or the end of the battle (which
    /// is then resolved).
    fn battle_play_ai(&mut self) {
        let Screen::Battle(b) = &mut self.screen else { return };
        let game = self.game.as_mut().expect("a game");
        for _ in 0..10_000 {
            // Every action is written back into the armies (0x4c4f8c, 0x4c57bc).
            game.battle_write_back(b);
            if b.outcome() != Outcome::Ongoing {
                break;
            }
            match b.active() {
                Some(a) if b.fighters[a].team == Team::Player => return,
                Some(_) => {
                    // A plan that cannot be carried out still ends the unit's turn.
                    match b.ai_step() {
                        Some(Step::Act { actor, hit }) => log_hit(&mut self.av, b, actor, &hit),
                        Some(Step::Move { actor, .. }) => log_move(&mut self.av, b, actor),
                        Some(Step::Wait { .. }) => {}
                        None => b.skip(),
                    }
                }
                None => break,
            }
        }
        if b.outcome() == Outcome::Ongoing {
            return;
        }
        let Screen::Battle(b) = std::mem::replace(&mut self.screen, Screen::Map) else { unreachable!() };
        self.finish_battle(&b);
    }

    /// The battle's end: its result applied, the result box or the end of the game.
    fn finish_battle(&mut self, b: &Battle) {
        let g = self.game.as_mut().expect("a game");
        let result = g.resolve_battle(b);
        let won = matches!(result, BattleResult::Victory { .. });
        // A win holds the battle screen 2.5 s with the experience on the cards, the triumph
        // starting at once, then the report follows (`battle_view`, 0x4c56a8); a level
        // gained has no sound there (`Unit-Upgrade` is the promotion screen's). A defeat
        // plays its piece; a battle nobody won shows Razdor's result box and its OK.
        self.triumph = won;
        match result {
            BattleResult::Victory { .. } => {
                self.av.anim("battle_end_hold", Some(format!("{}ms", av::BATTLE_END_HOLD_MS)));
                self.av.music(av::TRIUMPH);
            }
            BattleResult::Defeat => self.av.music(av::DEFEAT),
            BattleResult::Withdrew { .. } => self.av.sfx("InterfaceButtonDown"),
        }
        match result {
            BattleResult::Defeat => self.screen = Screen::Ended,
            BattleResult::Victory { .. } if g.won() => self.screen = Screen::Ended,
            // The victory box is the event window: it opens with its chord (0x4d165e).
            BattleResult::Victory { .. } => self.dialogs.push_back(Dialog::message()),
            BattleResult::Withdrew { .. } => {}
        }
    }

    /// The interface's handling of what happened (`world_view::handle_events`).
    fn handle(&mut self, events: Vec<Event>) {
        for e in events {
            match e {
                Event::Arrived(l) => {
                    let g = self.game.as_ref().expect("a game");
                    if g.foe.is_none() && g.village_offer().is_some() {
                        // The offer is a question in the event window, before any village window.
                        self.dialogs.push_back(Dialog { event: true, question: true, cued: false, offer: true, id: None });
                    } else if g.foe.is_none() && g.window_at(l).is_some() {
                        self.screen = Screen::Building;
                    }
                }
                Event::Tribute { .. } => self.tribute_due = true,
                // The noon report is the event window too: it opens with the chord.
                Event::NewDay(_) => self.dialogs.push_back(Dialog::message()),
                Event::Script(EventOutcome::Fired { message: true, event }) => self.dialogs.push_back(Dialog { id: Some(event), ..Dialog::message() }),
                Event::Script(EventOutcome::Question(event)) => self.dialogs.push_back(Dialog { event: true, question: true, cued: false, offer: false, id: Some(event) }),
                // `world_view::handle_events`: a spell read to its end lands (a level gained
                // from an event is silent: `Unit-Upgrade` is the promotion screen's).
                Event::SpellCast { target, outcome, .. } => log_landing(&mut self.av, target, outcome),
                _ => {}
            }
        }
    }

    /// The draws the interface makes as windows open (`App::sounds`): a village or shipyard
    /// window, then the front dialog if it is an event's.
    fn cue(&mut self) {
        let g = self.game.as_mut().expect("a game");
        let building = matches!(self.screen, Screen::Building);
        if building && !self.last_screen_building {
            let chord = g.location.is_some_and(|l| matches!(g.world.locations[l].kind, LocationKind::Village | LocationKind::Shipyard));
            if chord {
                let k = g.event_chord();
                self.ui_draws += 1;
                self.av.sfx(av::chord(k as u32));
            } else {
                // The building window opens on its first tab, highlighted (interface.md §14).
                self.av.sfx("InterfaceCastSpell");
            }
            self.tab = g.location.and_then(|l| g.window_at(l));
            self.selling = false;
        }
        if !building && self.last_screen_building {
            self.tab = None;
        }
        self.last_screen_building = building;
        if let Some(d) = self.dialogs.front_mut().filter(|d| !d.cued) {
            d.cued = true;
            if d.event {
                let k = g.event_chord();
                self.ui_draws += 1;
                self.av.sfx(av::chord(k as u32));
            } else {
                self.av.sfx("InterfacePanelDown");
            }
        }
    }

    /// The places an event showed: once that event's window is closed (before the next
    /// window opens) the camera flies to each and its uncovered cells fade in; after the
    /// event's last one it flies back to the hero (`world_view`, 0x4af96c, 0x4af83c).
    /// In a building window too: the window steps aside for the flights and comes back
    /// silent (`App::fly_from_building`, interface.md §9.8), so the log is the same.
    fn fly_to_shown(&mut self) {
        while let Some(&(event, _)) = self.shows.first() {
            if !self.free_to_show(event) {
                return;
            }
            let k = self.shows.iter().take_while(|s| s.0 == event).count();
            for (_, at) in self.shows.drain(..k).collect::<Vec<_>>() {
                let at = Some(format!("{},{}", at.0, at.1));
                self.av.anim("camera_glide", at.clone());
                self.av.anim("reveal", at);
            }
            let hero = self.g().tile();
            self.av.anim("camera_glide", Some(format!("{},{}", hero.0, hero.1)));
        }
    }

    /// The places of `event` may be shown: its window is closed (a place shown with no
    /// window waits for the windows before it).
    fn free_to_show(&self, event: Option<u16>) -> bool {
        match event {
            Some(id) if self.dialogs.iter().any(|d| d.id == Some(id)) => false,
            Some(_) => true,
            None => self.dialogs.is_empty(),
        }
    }

    /// Plays on until the game waits for input: events are handled, windows open, a pending
    /// fight opens its battle, a walk or a wait plays to its end.
    fn settle(&mut self) {
        for _ in 0..MAX_TICKS {
            if matches!(self.screen, Screen::Ended) {
                return;
            }
            let g = self.game.as_mut().expect("a game");
            // What the interface takes and has no place here.
            self.shows.extend(std::mem::take(&mut g.shown).into_iter().map(|s| (s.event, s.at)));
            g.autosave_due = None;
            // A map start or a load starts the world theme (`App::sounds`).
            if g.take_music_wait().is_some() {
                self.av.music(crate::rules::music::ROTATION[crate::rules::music::WORLD_THEME]);
            }
            let mut events = g.drain_events();
            let ticked = events.is_empty() && self.dialogs.is_empty() && matches!(self.screen, Screen::Map) && g.foe.is_none() && (g.moving() || g.wait_ticks > 0);
            if ticked {
                events = g.tick(STEP_SECONDS);
            }
            let any = !events.is_empty();
            self.handle(events);
            // An event window not on screen holds the scan no longer (the interface's net).
            let shown = self.dialogs.iter().any(|d| d.id.is_some() && !d.question);
            let released = self.g().release_unshown_window(shown);
            let any = any || !released.is_empty();
            self.handle(released);
            // The flights of a closed window's event come before the next window opens.
            self.fly_to_shown();
            self.cue();
            // The stop's snap, after the chords of the windows it opened (0x4ad8a0).
            self.g().armies_snap();
            let g = self.game.as_mut().expect("a game");
            if self.dialogs.is_empty() {
                if matches!(g.script_end(), Some(ScriptEnd::Victory(_) | ScriptEnd::Defeat(_))) || g.army_fallen() {
                    self.screen = Screen::Ended;
                    return;
                }
                if g.foe.is_some() && matches!(self.screen, Screen::Map | Screen::Building) {
                    // The horn as the battle window opens, and the battle's theme
                    // (`App::sounds`, `jukebox`: a garrison's or an army's).
                    let garrison = matches!(g.foe, Some(Foe::Garrison(_)));
                    self.av.sfx("Global-Battle");
                    self.av.music(av::BATTLE[if garrison { 0 } else { 1 }]);
                    let g = self.game.as_mut().expect("a game");
                    let mut b = g.start_battle();
                    b.begin();
                    self.screen = Screen::Battle(Box::new(b));
                    self.last_screen_building = false;
                    // The enemy's first moves, when it opens the battle.
                    self.battle_play_ai();
                    continue;
                }
            }
            if !any && !ticked {
                return;
            }
        }
        self.note("a walk or wait did not end".into());
    }

    /// The state now, as step `step`.
    pub fn state(&self, step: usize) -> Option<State> {
        let g = self.game.as_ref()?;
        let c = &g.content;
        let (x, y) = g.tile();
        let units = g.squad.iter().map(|u| HeroUnit { kind: u.def.0 as i32, level: u.level - 1, hp: u.hp.max(0), xp: u.xp, items: u.items.iter().map(|i| i.map_or(0, |i| i.0 as i32)).collect() }).collect();
        let pack = g.pack.iter().map(|i| i.0 as i32).collect();
        let book = g.spells.iter().map(|&s| i32::from(s)).collect();
        let hero = HeroState { x, y, gold: g.gold, mana: g.mana, units, pack, book };
        let w = &g.world;
        let army = |a: &Army, active: bool, alive: bool| {
            let (x, y) = a.tile(&w.map);
            ArmyState {
                id: a.id as i32,
                x: Some(x),
                y: Some(y),
                active: Some(active),
                alive,
                gold: Some(a.gold),
                units: Some(a.troops.iter().map(|t| ArmyUnit { kind: t.unit.0 as i32, level: t.level - 1, hp: troop_hp(c, t) }).collect()),
            }
        };
        let armies = match &self.scenario {
            None => w.armies.iter().map(|a| army(a, true, true)).collect(),
            Some(s) => s
                .armies
                .iter()
                .map(|r| {
                    let id = r.id;
                    if let Some(a) = w.armies.iter().find(|a| a.id == id) {
                        army(a, true, true)
                    } else if let Some(a) = w.inactive.iter().find(|a| a.id == id) {
                        army(a, false, true)
                    } else if let Some(r) = w.respawns.iter().find(|r| r.army.id == id) {
                        army(&r.army, false, false)
                    } else {
                        ArmyState { id: id as i32, x: None, y: None, active: None, alive: false, gold: None, units: None }
                    }
                })
                .collect(),
        };
        let buildings = w
            .locations
            .iter()
            .map(|l| BuildingState {
                id: l.id as i32,
                owner: match l.owner {
                    Owner::Player => 0,
                    Owner::Army(k) => k as i32,
                    Owner::Neutral => 255,
                },
                gold: l.tribute_gold,
                mana: l.tribute_mana,
                // A market's places; ruins keep their first goods as treasure; the map load
                // wipes the goods of every other building.
                goods: match &l.shop {
                    Some(s) => s.goods().iter().map(|i| i.0 as i32).collect(),
                    None if l.kind == LocationKind::Ruins => l.map_goods.clone(),
                    None => Vec::new(),
                },
            })
            .collect();
        let events_done = match (g.script(), &self.scenario) {
            (Some(e), Some(s)) => (1..=s.events.len() as u16).filter(|&id| e.times_fired(id) > 0).map(i32::from).collect(),
            _ => Vec::new(),
        };
        let battle = match &self.screen {
            Screen::Battle(b) => Some(battle_state(b)),
            _ => None,
        };
        Some(State { step, map: self.map.clone(), clock: g.clock.total_minutes() as u64, rng: g.rng.state(), hero, armies, buildings, events_done, battle })
    }
}

/// The battle as the state shows it.
/// A battle action as the battle window animates and sounds it (`battle_view`): the actor
/// lunges at the target (Razdor's stand-in for the original's slide, logged under its name),
/// the target shows the action's effect, its sound plays. A counterblow or a preventive
/// strike adds the target's lunge back and the same effect and sound on the actor; a
/// `DeathCurse` death of the killer the sorcery effect on it ([`av::echo`]).
fn log_hit(log: &mut AvLog, b: &Battle, actor: usize, hit: &crate::rules::battle::Hit) {
    log.sfx(BattleSound::of(b, actor, hit.kind).key());
    log.anim("battle_slide", Some(av::card(b, actor)));
    log.anim(format!("battle_effect:{}", av::battle_effect(hit.kind)), Some(av::card(b, hit.target)));
    match av::echo(b, actor, hit) {
        Some(av::Echo::Counter) => {
            log.sfx(BattleSound::of(b, actor, hit.kind).key());
            log.anim("battle_slide", Some(av::card(b, hit.target)));
            log.anim(format!("battle_effect:{}", av::battle_effect(hit.kind)), Some(av::card(b, actor)));
        }
        Some(av::Echo::Curse) => {
            log.sfx(BattleSound::Sorcery.key());
            log.anim("battle_effect:magic", Some(av::card(b, actor)));
        }
        None => {}
    }
}

/// A world spell lands (0x4af2f8): its effect on the army, and the good or evil sound when
/// it took effect.
fn log_landing(log: &mut AvLog, target: crate::rules::magic::CastTarget, outcome: crate::rules::magic::CastOutcome) {
    if matches!(outcome, crate::rules::magic::CastOutcome::Done { .. }) {
        log.anim("world_spell", None);
        log.sfx(if matches!(target, crate::rules::magic::CastTarget::Own) { "Spell-Good" } else { "Spell-Evil" });
    }
}

/// A step to another cell: the card slides there with `Card-Move`.
fn log_move(log: &mut AvLog, b: &Battle, actor: usize) {
    log.sfx("Card-Move");
    log.anim("card_slide", Some(av::card(b, actor)));
}

fn battle_state(b: &Battle) -> BattleState {
    let cell = |s: Slot| (s.row.number(), s.col as i32 + 1);
    let side = |team: Team| {
        b.fighters
            .iter()
            .filter(|f| f.team == team && f.listed())
            .map(|f| {
                let (row, col) = cell(f.slot);
                BattleUnit { kind: f.unit.0 as i32, row, col, hp: f.hp.max(0), actions: f.actions }
            })
            .collect()
    };
    let actor = b.active().map(|a| {
        let f = &b.fighters[a];
        let (row, col) = cell(f.slot);
        [if f.team == Team::Player { 1 } else { 2 }, row, col]
    });
    BattleState { turn: b.round, actor, sides: [side(Team::Player), side(Team::Enemy)] }
}

/// A troop's hit points: its maximum less what it lacks, 0 dead.
fn troop_hp(c: &Content, t: &Troop) -> i32 {
    if t.alive() {
        crate::rules::game::troop_unit(c, t).hp
    } else {
        0
    }
}

/// Reads an action list: one JSON object per line; blank lines and `#` comments skipped.
pub fn parse_actions(text: &str) -> Result<Vec<Action>, String> {
    text.lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
        .map(|(i, l)| serde_json::from_str(l).map_err(|e| format!("line {}: {e}", i + 1)))
        .collect()
}

/// Plays `actions` and returns the state after each one (step = the action's index) and the
/// notes on what could not be applied (each starts with its step).
pub fn replay(source: Source<'_>, actions: &[Action]) -> Result<(Vec<State>, Vec<String>), String> {
    let r = replay_traced(source, actions, None)?;
    Ok((r.states, r.notes))
}

/// What a replay gives.
pub struct Replay {
    pub states: Vec<State>,
    /// What could not be applied, each line starting with `step N:`.
    pub notes: Vec<String>,
    /// The generator's draws during each step (`draws[i]` for action `i`).
    pub draws: Vec<Vec<crate::rules::rng::trace::Draw>>,
    /// Facts outside the schema after each step (the battle units' stats), for the run log.
    pub raw: Vec<serde_json::Value>,
    /// What the screen shows after each step ([`Runner::look`]), for the explorer.
    pub looks: Vec<serde_json::Value>,
    /// The sounds, tracks and animations of each step ([`Runner::av`]).
    pub av: Vec<Vec<AvEvent>>,
}

/// [`replay`] with the generator's draws recorded step by step. With `rng_from`, each
/// step after the first starts with the generator set to `rng_from[step − 1]` (the
/// original's states, so that each step is compared from the same draws).
pub fn replay_traced(source: Source<'_>, actions: &[Action], rng_from: Option<&[u32]>) -> Result<Replay, String> {
    use crate::rules::rng::trace;
    let mut r = Runner::new(source);
    let mut out = Replay { states: Vec::new(), notes: Vec::new(), draws: Vec::new(), raw: Vec::new(), looks: Vec::new(), av: Vec::new() };
    trace::start();
    for (i, a) in actions.iter().enumerate() {
        if let Some(&state) = i.checked_sub(1).and_then(|k| rng_from?.get(k)) {
            r.set_rng(state);
        }
        let applied = r.apply(a).map_err(|e| format!("action {i}: {e}"));
        out.draws.push(trace::take());
        if let Err(e) = applied {
            trace::stop();
            return Err(e);
        }
        out.notes.extend(r.notes.drain(..).map(|n| format!("step {i}: {n}")));
        if let Some(s) = r.state(i) {
            out.states.push(s);
        }
        out.raw.push(r.raw());
        out.looks.push(r.look());
        out.av.push(r.av.take());
    }
    trace::stop();
    Ok(out)
}

/// The command line: `razdor --replay <actions.jsonl> [--map <file> --hero <1|2|3>] [--out
/// <dir>] [--rng-from <original.jsonl>] [--look]`. Returns `None` when `--replay` is not given (the game starts as usual), else the
/// exit code. `--map` puts a `new_game` before the list; the states go to
/// `<dir>/razdor.jsonl` (one line per step) or to the standard output.
pub fn cli(args: &[String]) -> Option<i32> {
    let k = args.iter().position(|a| a == "--replay")?;
    let value = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
    let Some(list) = args.get(k + 1) else {
        eprintln!("--replay needs an action list");
        return Some(2);
    };
    let look = args.iter().any(|a| a == "--look");
    match run_cli(Path::new(list), value("--map"), value("--hero"), value("--out").map(PathBuf::from), value("--rng-from").map(PathBuf::from), look) {
        Ok(()) => Some(0),
        Err(e) => {
            eprintln!("replay: {e}");
            Some(1)
        }
    }
}

/// The `rng` of each line of a state file (`original.jsonl`), by step.
fn rng_states(path: &Path) -> Result<Vec<u32>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut out = Vec::new();
    for (i, line) in text.lines().filter(|l| !l.trim().is_empty()).enumerate() {
        let v: serde_json::Value = serde_json::from_str(line).map_err(|e| format!("{}: line {}: {e}", path.display(), i + 1))?;
        let step = v["step"].as_u64().ok_or_else(|| format!("{}: line {}: no step", path.display(), i + 1))? as usize;
        let rng = v["rng"].as_u64().ok_or_else(|| format!("{}: line {}: no rng", path.display(), i + 1))? as u32;
        if step != out.len() {
            return Err(format!("{}: line {}: step {step}, expected {}", path.display(), i + 1, out.len()));
        }
        out.push(rng);
    }
    Ok(out)
}

/// Reads the action list of the command line (`--map` puts a `new_game` first).
pub fn read_action_list(list: &Path, map: Option<String>, hero: Option<String>) -> Result<Vec<Action>, String> {
    let text = std::fs::read_to_string(list).map_err(|e| format!("{}: {e}", list.display()))?;
    let mut actions = parse_actions(&text)?;
    if let Some(map) = map {
        let hero = hero.as_deref().unwrap_or("1").parse().map_err(|_| "--hero: 1, 2 or 3".to_string())?;
        actions.insert(0, Action::NewGame { map, hero, carry: None });
    }
    Ok(actions)
}

fn run_cli(list: &Path, map: Option<String>, hero: Option<String>, out: Option<PathBuf>, rng_from: Option<PathBuf>, look: bool) -> Result<(), String> {
    let actions = read_action_list(list, map, hero)?;
    let rng_from = rng_from.as_deref().map(rng_states).transpose()?;
    let needs_install = actions.iter().any(|a| matches!(a, Action::NewGame { map, .. } if map.trim() != "demo"));
    let dt = if needs_install {
        crate::dt::install::load_dotenv();
        let dir = crate::dt::install::locate().ok_or("no Discord Times install (set RAZDOR_DT_DIR)")?;
        Some(DtInstall::load(&dir).map_err(|e| format!("install {}: {e}", dir.display()))?)
    } else {
        None
    };
    let source = dt.as_ref().map_or(Source::Demo, Source::Install);
    let Replay { states, notes, draws, raw, looks, av } = replay_traced(source, &actions, rng_from.as_deref())?;
    for n in &notes {
        eprintln!("note: {n}");
    }
    let mut text = String::new();
    for s in &states {
        let mut v = serde_json::to_value(s).map_err(|e| e.to_string())?;
        // `--look`: what the screen shows, for the explorer (not part of the schema).
        if let Some(l) = looks.get(s.step).filter(|_| look) {
            v["look"] = l.clone();
        }
        text.push_str(&v.to_string());
        text.push('\n');
    }
    match out {
        Some(dir) => {
            std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            let path = dir.join("razdor.jsonl");
            std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
            // The notes and the generator's draws, step by step, for the differ.
            let mut extra = String::new();
            for (i, d) in draws.iter().enumerate() {
                let list: Vec<_> = d.iter().map(|d| serde_json::json!([d.n, d.before, format!("{}:{}", d.site.file(), d.site.line())])).collect();
                let step_notes: Vec<&String> = notes.iter().filter(|n| n.starts_with(&format!("step {i}: "))).collect();
                // `av`: the sounds, tracks and animations of the step (`crate::av`).
                extra.push_str(&serde_json::json!({ "step": i, "draws": list, "notes": step_notes, "meta": raw.get(i), "av": av.get(i) }).to_string());
                extra.push('\n');
            }
            let path = dir.join("razdor-run.jsonl");
            std::fs::write(&path, extra).map_err(|e| format!("{}: {e}", path.display()))?;
        }
        None => {
            let mut o = std::io::stdout().lock();
            o.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
