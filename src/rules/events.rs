//! Scenario events and quests: the engine of the original (`docs/reference/original-mechanics/
//! events.md`, record layout in `docs/reference/dtm-format.md` §9).
//!
//! Pure rules. The engine owns only the script state (what happened, flags, the journal, a
//! pending question); everything about the world goes through [`EventWorld`], which the game
//! implements. Texts are never copied into outcomes: an outcome names the event, and the UI
//! reads the title, question or message from the loaded scenario.
//!
//! The original's slips are kept and named where they are reproduced. Choices where the
//! sources are silent are marked *(guess)*.

use crate::dt::dtm::{BuildingType, Event, EventKind, Scenario};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::{BTreeSet, HashMap};

/// 1-based event id, in file order.
pub type EventId = u16;
/// 1-based army id.
pub type ArmyId = u8;

/// Owner / side codes of the editor: 1 player, 2–5 a faction (code − 1).
pub const SIDE_PLAYER: u8 = 1;
/// Condition owner code "not the player" (anyone else, or nobody).
pub const OWNER_NOT_PLAYER: u8 = 6;
/// Picture codes of byte 82.
pub const PICTURE_DEFEAT: u8 = 200;
pub const PICTURE_VICTORY: u8 = 201;

/// At most this many events fire in one [`EventEngine::tick`] (or answer, or hall entry);
/// more means a script loop. The original has no such guard (a Razdor safety net).
pub const LOOP_GUARD: usize = 256;
/// Chains deeper than this are cut (the original has no limit: a Razdor safety net).
const CHAIN_DEPTH: usize = 32;
/// The size of an event record: Community opcodes 1–5 address bytes across records.
const RECORD: i64 = 171;
/// The separator after each entry of the flag string (cp1251 non-breaking space).
const NBSP: u8 = 0xA0;
/// The digit a new counter flag starts with (0x4abf08).
const FIRST_DIGIT: u8 = b'1';

/// Where the player stands: in a building (1-based index in the scenario) or on an event point
/// (its point id).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Place {
    Building(u16),
    Point(u8),
}

/// The answer to an event's yes/no question. Events without a question count as `Yes`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Answer {
    Yes,
    No,
}

/// A Community Update extension found in an event (events.md §15).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Extension {
    /// "No meeting" = 1 and a patrol value ≠ 0: the patrol value (its byte; read signed) is
    /// an opcode, and the resource fields (and some condition fields) are its arguments.
    Opcode(u8),
}

/// A unit record as the event conditions and the unit results see it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UnitRecord {
    /// Unit type (the file's id).
    pub unit: u32,
    /// Named character (1-based), 0 none.
    pub named: u8,
    pub hp: i32,
    /// Joined through an event (wage kind 3).
    pub from_event: bool,
}

/// The original's unit search (Army_FindUnit 0x4961d0), from the last unit backwards, from
/// index `from` on: 0xFF the last unnamed unit after the leader; 0xFE the same, an event's
/// unit; any other type the last unnamed unit of that type when `named` is 0, or the unit
/// carrying `named` whatever its type.
pub fn find_unit(units: &[UnitRecord], pick: u8, named: u8, from: usize) -> Option<usize> {
    (from..units.len()).rev().find(|&i| {
        let u = &units[i];
        match pick {
            0xFF => i > 0 && u.named == 0,
            0xFE => i > 0 && u.named == 0 && u.from_event,
            t => (named == 0 && u.unit == t as u32 && u.named == 0) || (named != 0 && u.named == named),
        }
    })
}

/// The target of a Community opcode's first argument: the player's army (0), a scenario army
/// (its id, 1–255) or a building (a negative number: minus its 1-based index).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Holder {
    Player,
    Army(ArmyId),
    Building(u16),
}

impl Holder {
    pub fn from_code(v: i16) -> Option<Holder> {
        match v {
            0 => Some(Holder::Player),
            1..=255 => Some(Holder::Army(v as u8)),
            v if v < 0 => Some(Holder::Building(v.unsigned_abs())),
            _ => None,
        }
    }
}

/// Opcode 8's speed code as the editor's speed correction: 1 → +5 … 5 → +1, 6 → −1 … 8 → −3
/// and slower beyond; 0 → 0 *(guess: the guide gives only 1 → +5 and 8 → −3, so the code
/// skips 0)*.
pub fn speed_correction(code: i16) -> i8 {
    match code {
        ..=0 => 0,
        1..=5 => (6 - code) as i8,
        c => (5 - c as i32).max(-100) as i8,
    }
}

/// One "event editing" setting of opcodes 1–5: what to do (1 add, 2 set, 3–5 compare), to
/// which event (relative to the current one), which byte (its offset in the record; past
/// the record it reaches the next ones) and the value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EventEdit {
    pub action: i16,
    pub shift: i16,
    pub field: i16,
    pub value: i16,
}

/// The event editing settings of an opcode 1–5 event, as the editor offers them: the first
/// from the patrol value and the resources (XP = shift, gold = byte, mana = value), the
/// second, when its action (the squad count condition) is 1–5, from the conditions (gold =
/// shift, level = byte, holiness and mana = value).
pub fn event_edits(e: &Event) -> Vec<EventEdit> {
    let Some(Extension::Opcode(op @ 1..=5)) = extension(e) else { return Vec::new() };
    let [first, second] = edit_settings(e);
    let mut v = vec![EventEdit { action: op as i16, ..first }];
    if (1..=5).contains(&e.conditions.squad_count) {
        v.push(second);
    }
    v
}

/// Both settings as the patch reads them, whatever the opcode: the action of the first is
/// the patrol value, of the second the squad value's low byte, both read signed.
fn edit_settings(e: &Event) -> [EventEdit; 2] {
    let (r, c) = (&e.results, &e.conditions);
    [
        EventEdit { action: r.patrol_delta as i16, shift: r.experience, field: r.gold, value: r.mana },
        EventEdit { action: c.squad_count as i8 as i16, shift: c.gold, field: c.level, value: c.holiness_mana },
    ]
}

/// A field of an event record, addressed by its byte offset (dtm-format.md §9).
enum Field<'a> {
    U8(&'a mut u8),
    I8(&'a mut i8),
    U16(&'a mut u16),
    I16(&'a mut i16),
    U32(&'a mut u32),
}

impl Field<'_> {
    fn get(&self) -> i64 {
        match self {
            Field::U8(v) => **v as i64,
            Field::I8(v) => **v as i64,
            Field::U16(v) => **v as i64,
            Field::I16(v) => **v as i64,
            Field::U32(v) => **v as i64,
        }
    }

    /// Sets the value, clamped to the field's range.
    fn set(&mut self, x: i64) {
        match self {
            Field::U8(v) => **v = x.clamp(0, u8::MAX as i64) as u8,
            Field::I8(v) => **v = x.clamp(i8::MIN as i64, i8::MAX as i64) as i8,
            Field::U16(v) => **v = x.clamp(0, u16::MAX as i64) as u16,
            Field::I16(v) => **v = x.clamp(i16::MIN as i64, i16::MAX as i64) as i16,
            Field::U32(v) => **v = x.clamp(0, u32::MAX as i64) as u32,
        }
    }

    fn size(&self) -> u16 {
        match self {
            Field::U8(_) | Field::I8(_) => 1,
            Field::U16(_) | Field::I16(_) => 2,
            Field::U32(_) => 4,
        }
    }

    /// The field's bytes, little-endian, as the record stores them.
    fn bytes(&self) -> u32 {
        match self {
            Field::U8(v) => **v as u32,
            Field::I8(v) => **v as u8 as u32,
            Field::U16(v) => **v as u32,
            Field::I16(v) => **v as u16 as u32,
            Field::U32(v) => **v,
        }
    }

    /// The value the field holds when its bytes are `b`.
    fn value_of_bytes(&self, b: u32) -> i64 {
        match self {
            Field::U8(_) => b as u8 as i64,
            Field::I8(_) => b as u8 as i8 as i64,
            Field::U16(_) => b as u16 as i64,
            Field::I16(_) => b as u16 as i16 as i64,
            Field::U32(_) => b as i64,
        }
    }
}

/// The field starting at byte `off` of the event record; `None` inside a multi-byte field,
/// for unknown bytes and for the texts.
fn field(e: &mut Event, off: u16) -> Option<Field<'_>> {
    use Field::*;
    let (c, r) = (&mut e.conditions, &mut e.results);
    let o = off as usize;
    Some(match off {
        0 => U8(&mut e.group_colour),
        1 => U8(&mut e.kind),
        2 => U32(&mut e.start_time),
        6 => U16(&mut e.repeat),
        8 => U16(&mut e.duration),
        10 => U8(&mut e.archetype),
        11 => I16(&mut c.squad_count),
        13 => I16(&mut c.army_strength),
        15 => U8(&mut c.army_inactive),
        16 => U8(&mut r.patrol_army),
        17 => I8(&mut r.patrol_delta),
        18 => U8(&mut c.stats_check),
        19 => I16(&mut c.level),
        21 => I16(&mut c.gold),
        23..=24 => U8(&mut e.unknown_23[o - 23]),
        25 => I16(&mut c.holiness_mana),
        27..=28 => U8(&mut e.unknown_27[o - 27]),
        29 => U8(&mut c.buildings_check),
        30..=32 => U8(&mut c.buildings[o - 30]),
        33..=35 => U8(&mut c.buildings_owner[o - 33]),
        36 => U8(&mut c.units_check),
        37..=39 => U8(&mut c.units[o - 37]),
        40..=42 => U8(&mut c.units_named[o - 40]),
        43..=45 => U8(&mut c.units_owner[o - 43]),
        46 => U8(&mut c.artifacts_check),
        47..=49 => U8(&mut c.artifacts[o - 47]),
        50..=52 => U8(&mut c.artifacts_owner[o - 50]),
        53 => U8(&mut c.defeated_check),
        54..=55 => U8(&mut c.defeated_armies[o - 54]),
        56 => U8(&mut c.happened_yes_check),
        57 | 59 => U16(&mut c.happened_yes[(o - 57) / 2]),
        61 => U8(&mut c.not_happened_check),
        62 | 64 => U16(&mut c.not_happened[(o - 62) / 2]),
        66 => U8(&mut c.beaten_check),
        67..=68 => U8(&mut c.beaten_armies[o - 67]),
        69 => U8(&mut c.happened_no_check),
        70 | 72 => U16(&mut c.happened_no[(o - 70) / 2]),
        74 => U8(&mut c.meet_army),
        75 => U8(&mut c.army_active),
        76 => U8(&mut c.confirm_question),
        77 => U16(&mut r.relative_event),
        79 => U16(&mut r.relative_delay_hours),
        81 => U8(&mut r.cast_spell),
        82 => U8(&mut r.picture),
        83 => I16(&mut r.experience),
        85 => I16(&mut r.gold),
        87..=88 => U8(&mut e.unknown_87[o - 87]),
        89 => I16(&mut r.mana),
        91..=92 => U8(&mut e.unknown_91[o - 91]),
        93..=96 => U8(&mut r.spells_learned[o - 93]),
        97..=100 => U8(&mut r.units_add[o - 97]),
        101..=104 => U8(&mut r.units_add_named[o - 101]),
        105..=108 => U8(&mut r.units_remove[o - 105]),
        109..=112 => U8(&mut r.units_remove_named[o - 109]),
        113..=116 => U8(&mut r.artifacts_add[o - 113]),
        117..=120 => U8(&mut r.artifacts_remove[o - 117]),
        121..=122 => U8(&mut r.activate_armies[o - 121]),
        123 => U8(&mut r.deactivate_army),
        124 => U16(&mut r.completes_quest),
        126 => U16(&mut r.delay_hours),
        128 | 130 | 132 | 134 => U16(&mut r.light_lanterns[(o - 128) / 2]),
        136 => U8(&mut r.removed_units_to_army),
        137 => U8(&mut r.new_hero_class),
        138 => U16(&mut r.chained_event),
        140 => U8(&mut e.subordinate),
        141 => U8(&mut e.once),
        142 => U8(&mut r.units_from_army),
        143 => U8(&mut r.move_to_hero),
        144 => U8(&mut r.show_army),
        145 => U8(&mut r.hero_one_hp),
        146 => U8(&mut c.army_at_home),
        147 => U8(&mut r.start_battle_with),
        148 => U8(&mut r.no_meeting),
        149 => U8(&mut r.repeat_after_yes),
        150 => U8(&mut e.generate_battle_army),
        151..=155 => U8(&mut e.unknown_151[o - 151]),
        _ => return None,
    })
}

/// The field that holds byte `off` of the record: its start and the byte's place in it.
fn field_of_byte(e: &mut Event, off: u16) -> Option<(u16, u16)> {
    (0..4u16).filter(|k| *k <= off).find_map(|k| {
        let f = field(e, off - k)?;
        (k < f.size()).then_some((off - k, k))
    })
}

/// The value of the field at byte `off` of an event record (see [`EventEngine::event_field`]).
pub fn event_field(e: &Event, off: u16) -> Option<i64> {
    field(&mut e.clone(), off).map(|f| f.get())
}

/// What the game has to show or do after the engine ran. World effects (gold, units, armies,
/// …) have already been applied through [`EventWorld`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventOutcome {
    /// The event fired. `message`: it has a message to show (otherwise it fired silently).
    Fired { event: EventId, message: bool },
    /// The event asks its yes/no question; call [`EventEngine::answer`]. Nothing else runs
    /// until then.
    Question(EventId),
    /// The player answered No; the event did not take effect.
    Declined(EventId),
    QuestAdded(EventId),
    QuestCompleted(EventId),
    /// The scenario's victory or defeat event fired; the engine stops.
    Victory(EventId),
    Defeat(EventId),
    /// [`LOOP_GUARD`] events fired in one run; the rest waits for the next tick.
    LoopGuard,
}

/// The game as the events see it. Factions are 1–4 (the player's side, ally, neighbour,
/// enemy).
pub trait EventWorld {
    // Queries.
    /// Game time in minutes since year 0 (the scale of the event start times).
    fn now(&self) -> u64;
    /// 1 knight, 2 archmage, 3 ranger.
    fn hero_archetype(&self) -> u8;
    /// The hero's level as the original counts it, from 0.
    fn hero_level(&self) -> i64;
    fn hero_hp(&self) -> i64;
    fn gold(&self) -> i64;
    /// Holiness and mana.
    fn mana(&self) -> i64;
    /// Unit records in the player's army, the hero and the dead included.
    fn squad_count(&self) -> i64;
    fn army_strength(&self) -> i64;
    /// Building `building` (the 1-based index of the scenario's building): whether the
    /// player owns it, and its faction; `None` if there is no such building.
    fn building_state(&self, building: u16) -> Option<(bool, u8)>;
    /// The player's army, the hero first.
    fn player_units(&self) -> Vec<UnitRecord>;
    /// The armies of faction `faction`, each with its units (its leader first).
    fn faction_units(&self, faction: u8) -> Vec<Vec<UnitRecord>>;
    /// The player's items: the pack, and what his units wear.
    fn player_items(&self) -> (Vec<u8>, Vec<u8>);
    /// What the units of the armies of faction `faction` wear.
    fn faction_worn_items(&self, faction: u8) -> Vec<u8>;
    /// The player beat this army.
    fn player_defeated(&self, army: ArmyId) -> bool;
    /// It is destroyed, or someone beat it.
    fn army_beaten(&self, army: ArmyId) -> bool;
    /// On the map and not destroyed.
    fn army_active(&self, army: ArmyId) -> bool;
    /// Off the map and not destroyed.
    fn army_inactive(&self, army: ArmyId) -> bool;
    /// It stands in its home building, or it has none.
    fn army_at_home(&self, army: ArmyId) -> bool;
    fn place(&self) -> Option<Place>;

    // Effects.
    fn add_experience(&mut self, xp: i64);
    fn add_gold(&mut self, gold: i64);
    fn add_mana(&mut self, mana: i64);
    /// `named` 0 = an ordinary unit; `from_army` is the army it is taken from.
    fn add_unit(&mut self, unit: u8, named: u8, from_army: Option<ArmyId>);
    /// Unit `index` of the player's army leaves it, for `to_army` if given; `lead`: the
    /// removal's slot names a character, so the unit becomes that army's leader.
    fn remove_unit(&mut self, index: usize, lead: bool, to_army: Option<ArmyId>);
    fn give_item(&mut self, artifact: u8);
    fn take_item(&mut self, artifact: u8);
    fn learn_spell(&mut self, spell: u8);
    /// Cast a spell on the player's army.
    fn apply_spell(&mut self, spell: u8);
    fn activate_army(&mut self, army: ArmyId);
    fn deactivate_army(&mut self, army: ArmyId);
    fn show_army(&mut self, army: ArmyId);
    fn move_army_to_hero(&mut self, army: ArmyId);
    /// Light a lantern (point id): reveal its area.
    fn light_lantern(&mut self, point: u16);
    /// The places shown since the last call (the shown army, the lanterns) are event
    /// `event`'s: for the interface, which flies to them when its window closes.
    fn shown_by(&mut self, _event: EventId) {}
    fn change_patrol(&mut self, army: ArmyId, delta: i8);
    /// The hero becomes this unit type; class bonuses are lost.
    fn set_hero_class(&mut self, unit: u8);
    fn start_battle(&mut self, army: ArmyId);
    fn delay_player(&mut self, minutes: u64);

    // Community Update extensions. `unit` is a member of the holder's army in joining order
    // (0 = its leader, the hero for the player); `None` means everyone.
    /// Opcode 6: the unit wears exactly `items` (0 = an empty slot), fitting or not.
    fn equip_unit(&mut self, holder: Holder, unit: u8, items: [u8; 4]);
    /// Opcode 7: the unit becomes unit type `with`.
    fn replace_unit(&mut self, holder: Holder, unit: u8, with: u8);
    /// Opcode 8: the army's speed correction (the editor's −3..5).
    fn set_army_speed(&mut self, holder: Holder, correction: i8);
    /// Opcode 9: the army or building joins group 1 player, 2 ally, 3 neighbour, 4 enemy.
    fn set_faction(&mut self, holder: Holder, group: u8);
    /// Opcode 10: its relation (−3..3) towards group 0 player, 1 ally, 2 neighbour, 3 enemy.
    fn set_relation(&mut self, holder: Holder, group: u8, value: i8);
    /// Opcode 11: slot k of the unit(s) holds spell k of `spells` for good, or nothing for
    /// a 0.
    fn set_spells(&mut self, holder: Holder, unit: Option<u8>, spells: &[u8]);
    /// Opcode 12: the unit becomes the scenario's named character `named` (1-based) of unit
    /// type `class` (0: keep its type).
    fn set_named_unit(&mut self, holder: Holder, unit: u8, named: u8, class: u8);
    /// Opcode 13: experience for the unit(s).
    fn give_unit_xp(&mut self, holder: Holder, unit: Option<u8>, xp: i64);
    /// Opcode 14: all these spells last on the unit(s).
    fn has_spells(&self, holder: Holder, unit: Option<u8>, spells: &[u8]) -> bool;
    /// Opcode 16: the spell leaves the hero's spell book.
    fn forget_spell(&mut self, spell: u8);
    /// Opcode 17: the army's figure on the map (model 0–12).
    fn set_army_model(&mut self, holder: Holder, model: u8);
    /// Opcode 18: a random number in `lo..=hi`.
    fn random(&mut self, lo: i64, hi: i64) -> i64;
    /// Opcode 19: the AI army heads for cell (x, y).
    fn set_army_target(&mut self, army: ArmyId, x: i32, y: i32);
    /// Opcode 19 (condition): the army stands on cell (x, y).
    fn army_at(&self, army: ArmyId, x: i32, y: i32) -> bool;
    /// Opcode 20: the player's army moves to cell (x, y) and looks around.
    fn teleport_player(&mut self, x: i32, y: i32);
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct EventState {
    /// The last answer (`No` only after a No; opening the event again clears it).
    answer: Option<Answer>,
    /// Times the event fired, a No included (a No counts as happened).
    times: u32,
    /// Start time set by another event's "relative event" result.
    start: Option<u64>,
    /// The record's *last fired* (minutes): `now + 1` when it fires or is answered No, set
    /// back to now when the scan goes idle; `None` is the file's value (0).
    #[serde(default)]
    last_fired: Option<u64>,
    /// The record's answer byte when it is neither 0 nor 1 (from the file or an opcode's
    /// poke); `answer` then stays `None`.
    #[serde(default)]
    odd_answer: Option<u8>,
}

impl EventState {
    /// The record's answer byte (162): 1 after a No.
    fn answer_byte(&self) -> u8 {
        self.odd_answer.unwrap_or((self.answer == Some(Answer::No)) as u8)
    }

    fn set_answer_byte(&mut self, b: u8) {
        self.answer = (b == 1).then_some(Answer::No);
        self.odd_answer = (b > 1).then_some(b);
    }
}

/// The script state of a scenario: see the module docs. A save keeps the state only; the
/// events and places come from the scenario again ([`EventEngine::restore_statics`]).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EventEngine {
    #[serde(skip)]
    events: Vec<Event>,
    state: Vec<EventState>,
    #[serde(skip)]
    places: HashMap<Place, Vec<EventId>>,
    /// Villages and shipyards: every event they list fires on entering, whatever its kind.
    #[serde(skip)]
    every_kind: BTreeSet<Place>,
    /// The flag string as the original keeps it (0x68ed00), cp1251 bytes: each entry a name,
    /// a counter's digit for the `^` form, and a non-breaking space. Saves hold it as text
    /// (older saves as a list of names, read into the string).
    #[serde(serialize_with = "flags_out", deserialize_with = "flags_in")]
    flags: Vec<u8>,
    /// The quest journal (0x68e500): a quest is added every time it finishes.
    journal: Vec<EventId>,
    /// Quests completed, in order (a Razdor record for the journal's history).
    completed: Vec<EventId>,
    #[serde(skip)]
    victory: EventId,
    #[serde(skip)]
    defeat: EventId,
    pending: Option<EventId>,
    last_place: Option<Place>,
    /// The hero just entered the building he stands in: its events are checked in this run
    /// only (the original checks a building's events while it is not the one the hero was in
    /// when his move began).
    #[serde(default)]
    fresh_visit: bool,
    /// The army the hero is meeting ([`EventEngine::meet`]): "meet army" holds for it until
    /// the run that followed the meeting ends or a "no meeting" result clears it.
    #[serde(default)]
    meeting: Option<ArmyId>,
    ended: Option<EventOutcome>,
    #[serde(skip)]
    extensions: Vec<(EventId, Extension)>,
    /// Fields of events changed at run time (event, byte offset, value): the ask and once
    /// bytes a Yes rewrites, and Community opcodes 1–2's pokes. Put back into the events
    /// when a save is loaded.
    #[serde(default)]
    edits: Vec<(EventId, u16, i64)>,
    /// Community opcode 15: the campaign branch chosen (map number, variant).
    #[serde(default)]
    branch: Option<(i16, i16)>,
    /// Community opcode 18's digit, waiting for the next `^` flag action (c286af, c286b0).
    #[serde(default)]
    random_digit: Option<u8>,
    /// An event whose test holds `end_tutorial` finished (0x4ac9fc).
    #[serde(default)]
    tutorial_done: bool,
    /// An event's window is shown: the scan stopped at it (0x4ac3b4 opens the window and
    /// the scan returns) and goes on only when it is closed ([`EventEngine::window_closed`]:
    /// OK → Event_Finish 0x4ab1ec → its chain or a new scan). Not saved: a loaded game
    /// scans afresh.
    #[serde(skip)]
    held: bool,
    /// The chained event of a shown event, opened when its window is closed (the chain timer
    /// 0x4af658 that Event_Finish starts).
    #[serde(skip)]
    held_chain: Option<EventId>,
    /// The unit type of each named character (1-based), for opcode 12.
    #[serde(skip)]
    named_units: Vec<u8>,
    /// The scenario's next map (campaigns) and what carries over to it (header 0x110).
    #[serde(skip)]
    next_map: String,
    #[serde(skip)]
    carry_over: [u8; 7],
}

fn flags_out<S: Serializer>(flags: &[u8], s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&crate::dt::text::decode(flags))
}

/// The flag string of a save: text, or (saves before format 8) a list of flag names.
fn flags_in<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Saved {
        Text(String),
        Names(Vec<String>),
    }
    Ok(match Saved::deserialize(d)? {
        Saved::Text(t) => crate::dt::text::encode(&t),
        Saved::Names(names) => names
            .iter()
            .flat_map(|n| {
                let mut b = crate::dt::text::encode(n);
                b.push(NBSP);
                b
            })
            .collect(),
    })
}

/// Delphi's `Pos`: the first place of `needle` in `hay`; an empty needle is never found.
fn pos(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || needle.len() > hay.len() {
        return None;
    }
    hay.windows(needle.len()).position(|w| w == needle)
}

/// Minutes in a day: an event's repeat counts in whole days.
const DAY: i64 = 1440;

/// Whether the time window is open at `now` (events.md §3): never before the start (a
/// negative start counts as 0); with a repeat of R minutes, on every (R div 1440)-th day since
/// the start, for D = max(duration, 1) hours from the start's time of day (the stored
/// duration is taken as hours); without one, until start + D hours, or with no end for
/// duration 0 while the event never fired. A repeat under a day divides by zero in the
/// original (unknown); Razdor takes it as every day.
fn window_open(start: i64, repeat: u64, duration: u64, fired: bool, now: i64) -> bool {
    let start = start.max(0);
    if start > now {
        return false;
    }
    let length = duration.max(1) as i64 * 60;
    let k = (now - start) / DAY;
    let day = start + DAY * k;
    let every = (repeat as i64 / DAY).max(1);
    (repeat != 0 && k % every == 0 && day <= now && now <= day + length)
        || (repeat == 0 && now <= start + length)
        || (repeat == 0 && duration == 0 && !fired)
}

/// The sign encodes ≥ (positive) or ≤ (negative); 0 disables the check (0x4a7b40).
fn compare(value: i64, threshold: i16) -> bool {
    match threshold {
        0 => true,
        t if t > 0 => value >= t as i64,
        t => value <= -(t as i64),
    }
}

fn nonzero<T: Copy + Default + PartialEq>(v: &[T]) -> impl Iterator<Item = T> + '_ {
    v.iter().copied().filter(|x| *x != T::default())
}

/// The Community extension an event uses, if any: opcode mode is "no meeting" = 1 with a
/// patrol value ≠ 0 (0xc2669e, 0xc27862).
pub fn extension(e: &Event) -> Option<Extension> {
    let r = &e.results;
    (r.no_meeting == 1 && r.patrol_delta != 0).then_some(Extension::Opcode(r.patrol_delta as u8))
}

/// The opcode of an event in opcode mode, read signed.
fn opcode(e: &Event) -> Option<i8> {
    extension(e).map(|Extension::Opcode(op)| op as i8)
}

/// A condition that failed, and whether it comes after the meet-army test (only those leave
/// an army marked "meeting event waiting", 0x4a801a).
struct Failed {
    name: &'static str,
    after_meeting: bool,
}

/// How an event ends: what its finish queued for the follow-ups (0x4ab1ec tail).
#[derive(Default)]
struct Queued {
    battle: ArmyId,
    spell: u8,
    delay: u16,
    /// Lanterns lit or an army shown: their animations start the chain timer.
    shown: bool,
}

impl EventEngine {
    /// The engine for a scenario: its events (texts as the game loads them), the events of
    /// its buildings and points, and its victory and defeat events.
    pub fn new(s: &Scenario) -> EventEngine {
        let mut places = Vec::new();
        for (i, b) in s.buildings.iter().enumerate() {
            places.push((Place::Building(i as u16 + 1), b.events().collect()));
        }
        for p in &s.points {
            places.push((Place::Point(p.id), p.events().collect()));
        }
        let events = s.events.iter().map(Event::for_play).collect();
        let mut g = EventEngine::from_parts(events, places, s.header.victory_event, s.header.defeat_event);
        for (i, b) in s.buildings.iter().enumerate() {
            if matches!(b.building_type(), Some(BuildingType::Village | BuildingType::Shipyard)) {
                g.set_every_kind(Place::Building(i as u16 + 1));
            }
        }
        g.named_units = s.named_characters.iter().map(|n| n.unit).collect();
        g.next_map = s.next_map.clone();
        g.carry_over = s.header.carry_over;
        g
    }

    /// The engine for hand-made events. `places` lists the events of each place.
    pub fn from_parts(
        events: Vec<Event>,
        places: Vec<(Place, Vec<EventId>)>,
        victory: EventId,
        defeat: EventId,
    ) -> EventEngine {
        let n = events.len();
        let mut map: HashMap<Place, Vec<EventId>> = HashMap::new();
        for (p, ids) in places {
            let list = map.entry(p).or_default();
            for id in ids {
                if (1..=n).contains(&(id as usize)) && !list.contains(&id) {
                    list.push(id);
                }
            }
        }
        let extensions = Self::extensions_of(&events);
        // The record carries the engine's state from the file (0x4b2504).
        let state = events
            .iter()
            .map(|e| {
                let (last, times, answer) = e.runtime_state();
                let mut st = EventState {
                    times: times as u32,
                    last_fired: (last != 0).then_some(last.max(0) as u64),
                    ..EventState::default()
                };
                st.set_answer_byte(answer);
                st
            })
            .collect();
        EventEngine {
            state,
            events,
            places: map,
            every_kind: BTreeSet::new(),
            flags: Vec::new(),
            journal: Vec::new(),
            completed: Vec::new(),
            victory,
            defeat,
            pending: None,
            last_place: None,
            fresh_visit: false,
            meeting: None,
            ended: None,
            extensions,
            edits: Vec::new(),
            branch: None,
            random_digit: None,
            tutorial_done: false,
            held: false,
            held_chain: None,
            named_units: Vec::new(),
            next_map: String::new(),
            carry_over: [0; 7],
        }
    }

    /// In building `place` every listed event fires on entering, whatever its kind, as in
    /// the original's villages and shipyards (4ac1a1).
    pub fn set_every_kind(&mut self, place: Place) {
        self.every_kind.insert(place);
    }

    /// Unit types of the scenario's named characters, in order (hand-made engines).
    pub fn set_named_units(&mut self, units: Vec<u8>) {
        self.named_units = units;
    }

    /// The next map of a campaign, as the scenario names it (may be empty).
    pub fn next_map_name(&self) -> &str {
        &self.next_map
    }

    /// Sets the scenario's next map and carry-over flags (hand-made engines).
    pub fn set_next_map(&mut self, name: &str, carry_over: [u8; 7]) {
        self.next_map = name.to_string();
        self.carry_over = carry_over;
    }

    /// What carries over to the next map (header 0x110, in UI order: gold, gods' favour,
    /// fame, experience/level, personal artifacts, whole inventory, whole army).
    pub fn carry_over(&self) -> [u8; 7] {
        self.carry_over
    }

    /// The campaign branch an opcode 15 event chose: (map number, variant).
    pub fn campaign_branch(&self) -> Option<(i16, i16)> {
        self.branch
    }

    /// An event marked the tutorial done (its test holds `end_tutorial`, 0x4ac9fc).
    pub fn tutorial_done(&self) -> bool {
        self.tutorial_done
    }

    /// The value of the field at byte `off` of event `id` as it stands now (Community
    /// opcodes 1–5 read and change these).
    pub fn event_field(&self, id: EventId, off: u16) -> Option<i64> {
        event_field(self.event(id)?, off)
    }

    /// Sets the field at byte `off` of event `id` (clamped to its range) and records it for
    /// saves.
    fn set_event_field(&mut self, id: EventId, off: u16, v: i64) {
        let Some(e) = (id as usize).checked_sub(1).and_then(|i| self.events.get_mut(i)) else { return };
        let Some(mut f) = field(e, off) else { return };
        f.set(v);
        let now = f.get();
        match self.edits.iter_mut().find(|(i, o, _)| (*i, *o) == (id, off)) {
            Some(edit) => edit.2 = now,
            None => self.edits.push((id, off, now)),
        }
        self.extensions = Self::extensions_of(&self.events);
    }

    /// The record byte at `at` counted from the start of event 1's record, as opcodes 1–5
    /// address it (a byte past one record is in the next): (event, byte offset).
    fn byte_at(&self, at: i64) -> Option<(EventId, u16)> {
        let (i, off) = (at.div_euclid(RECORD), at.rem_euclid(RECORD));
        let id = EventId::try_from(i + 1).ok()?;
        self.event(id).map(|_| (id, off as u16))
    }

    /// A byte of an event record as it stands now: the engine's state for bytes 156–162
    /// (last fired, times fired, the answer); `None` for a byte Razdor does not model (the
    /// picture's size and pointer) *(guess: such a poke or test is ignored)*.
    fn record_byte(&self, id: EventId, off: u16) -> Option<u8> {
        if let Some(k) = off.checked_sub(156).filter(|k| *k < 7) {
            self.event(id)?;
            let st = self.st(id);
            let last = st.last_fired.unwrap_or(0) as u32;
            let times = st.times as u16;
            let answer = st.answer_byte();
            return Some(match k {
                0..=3 => (last >> (8 * k)) as u8,
                4 | 5 => (times >> (8 * (k - 4))) as u8,
                _ => answer,
            });
        }
        let mut e = self.event(id)?.clone();
        let (start, k) = field_of_byte(&mut e, off)?;
        let f = field(&mut e, start)?;
        Some((f.bytes() >> (8 * k)) as u8)
    }

    /// Writes one byte of an event record (opcodes 1–2 poke bytes, not fields); bytes
    /// 156–162 are the engine's state, which a poke changes like any other byte.
    fn set_record_byte(&mut self, id: EventId, off: u16, b: u8) {
        if let Some(k) = off.checked_sub(156).filter(|k| *k < 7) {
            if self.event(id).is_none() {
                return;
            }
            let st = self.st_mut(id);
            match k {
                0..=3 => {
                    let mask = 0xFFu32 << (8 * k);
                    let last = (st.last_fired.unwrap_or(0) as u32 & !mask) | ((b as u32) << (8 * k));
                    // The record holds a signed minute; a negative one is as good as 0 here.
                    st.last_fired = Some((last as i32).max(0) as u64);
                }
                4 | 5 => {
                    let shift = 8 * (k - 4);
                    let times = (st.times as u16 & !(0xFF << shift)) | ((b as u16) << shift);
                    st.times = times as u32;
                }
                _ => st.set_answer_byte(b),
            }
            return;
        }
        let Some(mut e) = self.event(id).cloned() else { return };
        let Some((start, k)) = field_of_byte(&mut e, off) else { return };
        let Some(f) = field(&mut e, start) else { return };
        let mask = 0xFFu32 << (8 * k);
        let v = f.value_of_bytes((f.bytes() & !mask) | ((b as u32) << (8 * k)));
        self.set_event_field(id, start, v);
    }

    fn extensions_of(events: &[Event]) -> Vec<(EventId, Extension)> {
        events.iter().enumerate().filter_map(|(i, e)| Some((i as EventId + 1, extension(e)?))).collect()
    }

    /// Puts back what a save leaves out (the events, places, victory and defeat events)
    /// from `fresh`, the engine of the same scenario. Fails if the event count differs.
    pub fn restore_statics(&mut self, fresh: EventEngine) -> Result<(), String> {
        if self.state.len() != fresh.events.len() {
            return Err(format!("{} events saved, the map has {}", self.state.len(), fresh.events.len()));
        }
        self.events = fresh.events;
        self.places = fresh.places;
        self.every_kind = fresh.every_kind;
        self.victory = fresh.victory;
        self.defeat = fresh.defeat;
        self.named_units = fresh.named_units;
        self.next_map = fresh.next_map;
        self.carry_over = fresh.carry_over;
        for (id, off, v) in std::mem::take(&mut self.edits) {
            self.set_event_field(id, off, v);
        }
        self.extensions = Self::extensions_of(&self.events);
        Ok(())
    }

    // ---------------------------------------------------------------------------------------
    // State
    // ---------------------------------------------------------------------------------------

    pub fn event(&self, id: EventId) -> Option<&Event> {
        (id as usize).checked_sub(1).and_then(|i| self.events.get(i))
    }

    fn ev(&self, id: EventId) -> &Event {
        &self.events[id as usize - 1]
    }

    fn st(&self, id: EventId) -> &EventState {
        &self.state[id as usize - 1]
    }

    fn st_mut(&mut self, id: EventId) -> &mut EventState {
        &mut self.state[id as usize - 1]
    }

    /// Whether the event happened, and how: `No` after a No answer, `Yes` otherwise;
    /// `None` if it never fired (a No counts as happened).
    pub fn happened(&self, id: EventId) -> Option<Answer> {
        self.event(id)?;
        let st = self.st(id);
        match (st.times, st.answer) {
            (0, _) => None,
            (_, Some(Answer::No)) => Some(Answer::No),
            _ => Some(Answer::Yes),
        }
    }

    /// The events listed at a place (a building or an event point), in the map's order.
    pub fn events_at(&self, place: Place) -> &[EventId] {
        self.places.get(&place).map_or(&[], Vec::as_slice)
    }

    /// How many times the event fired (a No answer included).
    pub fn times_fired(&self, id: EventId) -> u32 {
        self.event(id).map_or(0, |_| self.st(id).times)
    }

    /// The minute the event last fired (or was answered No): the record's *last fired*,
    /// which is a minute ahead until the scan goes idle.
    pub fn last_fired(&self, id: EventId) -> Option<u64> {
        self.event(id).and_then(|_| self.st(id).last_fired)
    }

    /// Total firings over all events.
    pub fn total_fired(&self) -> u32 {
        self.state.iter().map(|s| s.times).sum()
    }

    /// Whether a flag test holds: `X` when X occurs anywhere in the flag string, `/X` when it
    /// does not (events.md §5). The test is a substring search, not a name lookup, as in the
    /// original: `AB` holds while `XAB1` is set.
    pub fn flag(&self, test: &str) -> bool {
        self.flag_test(test)
    }

    /// The flag string as text (entries end in a non-breaking space).
    pub fn flag_string(&self) -> String {
        crate::dt::text::decode(&self.flags)
    }

    /// Sets the flag string (a campaign map starts with the last map's).
    pub fn set_flag_string(&mut self, flags: &str) {
        self.flags = crate::dt::text::encode(flags);
    }

    /// The names in the flag string, for display.
    pub fn flags(&self) -> Vec<String> {
        self.flags.split(|&b| b == NBSP).filter(|n| !n.is_empty()).map(crate::dt::text::decode).collect()
    }

    /// The test part of a title script (0x4a8a22): the first `^` removed; empty, or holding
    /// `end_tutorial`, it passes; with a `/` (removed) it passes when the rest does not occur
    /// in the flag string, else when it does.
    fn flag_test(&self, test: &str) -> bool {
        let mut s = crate::dt::text::encode(test);
        if let Some(i) = s.iter().position(|&b| b == b'^') {
            s.remove(i);
        }
        if s.is_empty() || pos(&s, b"end_tutorial").is_some() {
            return true;
        }
        match s.iter().position(|&b| b == b'/') {
            Some(i) => {
                s.remove(i);
                pos(&self.flags, &s).is_none()
            }
            None => pos(&self.flags, &s).is_some(),
        }
    }

    /// The action part of a title script (0x4ab2a3). Two characters or fewer do nothing.
    /// `+X` appends X when it does not occur yet; `-X` removes the first occurrence and the
    /// character after it. The `^` form keeps a digit: `+X^` appends X1, or raises the
    /// character after X; `-X^` lowers it and removes the entry at 0. As in the original,
    /// the `^` is taken to be the last character. Razdor fixes the original's bug: its `-X^`
    /// did not check that X occurs (it then lowered the character at X's length).
    fn flag_action(&mut self, action: &str) {
        let a = crate::dt::text::encode(action);
        if a.len() <= 2 {
            return;
        }
        let counter = a.contains(&b'^');
        let mut s = a[1..].to_vec();
        if counter {
            s.pop();
        }
        let p = pos(&self.flags, &s);
        match a[0] {
            b'+' if counter => match p {
                None => {
                    let d = self.random_digit.take().unwrap_or(FIRST_DIGIT);
                    self.flags.extend_from_slice(&s);
                    self.flags.extend([d, NBSP]);
                }
                Some(p) => {
                    let d = self.random_digit.take();
                    if let Some(c) = self.flags.get_mut(p + s.len()) {
                        *c = d.unwrap_or(c.wrapping_add(1));
                    }
                }
            },
            b'+' => {
                if p.is_none() {
                    self.flags.extend_from_slice(&s);
                    self.flags.push(NBSP);
                }
            }
            b'-' if self.flags.is_empty() => {}
            b'-' if counter => {
                let Some(p) = p else { return };
                if let Some(c) = self.flags.get_mut(p + s.len()) {
                    *c = c.wrapping_sub(1);
                    if *c == b'0' {
                        let end = (p + s.len() + 2).min(self.flags.len());
                        self.flags.drain(p..end);
                    }
                }
            }
            b'-' => {
                if let Some(p) = p {
                    let end = (p + s.len() + 1).min(self.flags.len());
                    self.flags.drain(p..end);
                }
            }
            _ => {}
        }
    }

    /// The engine's own flags (0x496d28, 0x497c68): `Sea` while the hero is aboard,
    /// `EnterShipyard` while he is in his ship's shipyard; added and removed like a script's
    /// `+X` and `-X`.
    pub fn set_engine_flag(&mut self, name: &str, on: bool) {
        let sign = if on { '+' } else { '-' };
        self.flag_action(&format!("{sign}{name}"));
    }

    /// The quest journal, in the order received: a quest is listed again every time it
    /// finishes.
    pub fn journal(&self) -> &[EventId] {
        &self.journal
    }

    pub fn completed_quests(&self) -> &[EventId] {
        &self.completed
    }

    /// The event whose question waits for [`EventEngine::answer`].
    pub fn pending_question(&self) -> Option<EventId> {
        self.pending
    }

    /// `Victory` or `Defeat` once the scenario is over.
    pub fn ended(&self) -> Option<&EventOutcome> {
        self.ended.as_ref()
    }

    /// Ends the scenario as `end` would have, for tests of what follows an end.
    #[cfg(test)]
    pub(crate) fn end_for_test(&mut self, end: EventOutcome) {
        self.ended = Some(end);
    }

    /// Community extensions in the scenario.
    pub fn extensions(&self) -> &[(EventId, Extension)] {
        &self.extensions
    }

    /// The start of an event's window: its own, or the one another event set.
    pub fn start_time(&self, id: EventId) -> Option<u64> {
        let e = self.event(id)?;
        Some(self.st(id).start.unwrap_or(e.start_time as u64))
    }

    // ---------------------------------------------------------------------------------------
    // Entry points
    // ---------------------------------------------------------------------------------------

    /// Run the scan: take the first event that passes, again until none does. Call it
    /// whenever time passes or the player arrives somewhere.
    pub fn tick(&mut self, w: &mut dyn EventWorld) -> Vec<EventOutcome> {
        let mut out = Vec::new();
        let place = w.place();
        if place != self.last_place {
            self.last_place = place;
            if let Some(p) = place {
                self.visit(p);
            }
        }
        self.run(w, &mut out);
        out
    }

    /// The hero enters building `place` (again): its events are checked in the next run.
    pub fn visit(&mut self, place: Place) {
        self.fresh_visit = matches!(place, Place::Building(_));
    }

    /// The hero meets army `army` (on the road, or it caught him): the events run with the
    /// meeting in force (the original's current meeting, 0x68dc7c).
    pub fn meet(&mut self, w: &mut dyn EventWorld, army: ArmyId) -> Vec<EventOutcome> {
        self.meeting = Some(army);
        self.tick(w)
    }

    /// The army being met, while the run after the meeting goes on.
    pub fn meeting(&self) -> Option<ArmyId> {
        self.meeting
    }

    /// Answer the pending question, then run the events on (0x4c2100, 0x4c2320).
    pub fn answer(&mut self, w: &mut dyn EventWorld, yes: bool) -> Vec<EventOutcome> {
        let mut out = Vec::new();
        let Some(id) = self.pending.take() else { return out };
        if !yes {
            // No: the answer, the guard and the count, nothing else (no flag, result, chain
            // or quest). It counts as happened and uses up a once-event.
            let now = w.now();
            let st = self.st_mut(id);
            st.set_answer_byte(1);
            st.last_fired = Some(now + 1);
            st.times += 1;
            out.push(EventOutcome::Declined(id));
        } else {
            // Yes: the ask byte is cleared, and once := "repeat after yes" (149) xor 1, so
            // without 149 every answered event becomes a once-event, whatever its box said.
            // As in the original it is a bit flip, not a logical not: a 149 of 2 gives once 3.
            let again = self.ev(id).results.repeat_after_yes;
            self.set_event_field(id, 76, 0);
            self.set_event_field(id, 141, (again ^ 1) as i64);
            if self.ev(id).message.is_empty() {
                // No message: finished at once, its artifacts, units and spells never
                // applied; ask is set back for later firings. Razdor fixes the original's
                // bug: its "ask again" write landed outside the event table (the event was
                // closed), so a repeating event kept ask = 0 and fired without its question.
                self.set_event_field(id, 76, 1);
                out.push(EventOutcome::Fired { event: id, message: false });
                self.finish(id, w, &mut out, 0);
            } else {
                // The window opens again without the question (the dialog-time results, the
                // message); ask is set back for later firings; OK finishes.
                self.set_event_field(id, 76, 1);
                self.show(id, w, &mut out);
                self.finish(id, w, &mut out, 0);
            }
        }
        self.run(w, &mut out);
        out
    }

    /// The window of the shown event was closed: its chain, then the scan goes on (0x4c206c
    /// OK → Event_Finish 0x4ab1ec → the chain timer or a new scan). Nothing when no event's
    /// window holds the scan.
    pub fn window_closed(&mut self, w: &mut dyn EventWorld) -> Vec<EventOutcome> {
        let mut out = Vec::new();
        if !std::mem::take(&mut self.held) {
            return out;
        }
        if let Some(next) = self.held_chain.take() {
            if self.ended.is_none() {
                self.open(next, w, &mut out, 1);
            }
        }
        self.run(w, &mut out);
        out
    }

    /// An event's window is shown and holds the scan.
    pub fn holds_window(&self) -> bool {
        self.held
    }

    /// The main hall's list where the hero stands (0x4beaac): the building's quests and
    /// rumours that pass the full check, in its list order. Villages, shipyards and event
    /// points have none (the scan fires their quests and rumours).
    pub fn hall(&self, w: &dyn EventWorld) -> Vec<EventId> {
        let Some(place @ Place::Building(_)) = w.place() else { return Vec::new() };
        if self.every_kind.contains(&place) {
            return Vec::new();
        }
        let ids = self.places.get(&place).map(Vec::as_slice).unwrap_or_default();
        ids.iter()
            .copied()
            .filter(|&id| matches!(self.ev(id).kind(), Some(EventKind::Quest | EventKind::Rumour)) && self.passes(id, w))
            .collect()
    }

    /// The player takes an entry of the hall's list: its dialog opens at once, without a new
    /// check (0x4bb798), then the events run on.
    pub fn take(&mut self, w: &mut dyn EventWorld, id: EventId) -> Vec<EventOutcome> {
        let mut out = Vec::new();
        if self.pending.is_none() && self.ended.is_none() && self.hall(w).contains(&id) {
            // The list's check (with apply) cleared the ask of an event without texts.
            let e = self.ev(id);
            if e.conditions.confirm_question != 0 && e.question.is_empty() && e.message.is_empty() {
                self.set_event_field(id, 76, 0);
            }
            self.open(id, w, &mut out, 0);
            self.run(w, &mut out);
        }
        out
    }

    // ---------------------------------------------------------------------------------------
    // The scan
    // ---------------------------------------------------------------------------------------

    fn run(&mut self, w: &mut dyn EventWorld, out: &mut Vec<EventOutcome>) {
        let mut fired = 0;
        while self.pending.is_none() && self.ended.is_none() && !self.held {
            let Some(id) = self.first_eligible(w) else { break };
            if fired == LOOP_GUARD {
                out.push(EventOutcome::LoopGuard);
                break;
            }
            fired += 1;
            // A passing event with neither a question nor a message never asks (0x4a7a40).
            let e = self.ev(id);
            if e.conditions.confirm_question != 0 && e.question.is_empty() && e.message.is_empty() {
                self.set_event_field(id, 76, 0);
            }
            self.open(id, w, out, 0);
        }
        if self.pending.is_none() && !self.held {
            // The scan goes idle (0x4ac369): a last fired in the future is set back to now,
            // so the guard only holds within one scan; the meeting is over (0x4ac3a0).
            let now = w.now();
            for st in &mut self.state {
                if st.last_fired.is_some_and(|l| l > now) {
                    st.last_fired = Some(now);
                }
            }
            self.fresh_visit = false;
            self.meeting = None;
        }
    }

    /// The firing guard on *last fired* L (0x4a7b80): with a duration, L ≤ now; without one,
    /// L < now − 60.
    fn may_refire(&self, id: EventId, now: u64) -> bool {
        let l = self.st(id).last_fired.unwrap_or(0) as i64;
        match self.ev(id).duration {
            0 => l < now as i64 - 60,
            _ => l <= now as i64,
        }
    }

    /// The time window is open at `now` ([`window_open`]).
    fn is_open(&self, id: EventId, now: u64) -> bool {
        let e = self.ev(id);
        let st = self.st(id);
        // The start is a signed 32-bit number in the record.
        let start = st.start.map_or(e.start_time as i32 as i64, |s| s as i64);
        window_open(start, e.repeat as u64, e.duration as u64, st.times > 0, now as i64)
    }

    /// Eligibility, in the original's order (0x4a7b80): not done (byte 140, the editor's
    /// "subordinate event": such an event runs only as a chain), the guard, the window, not a
    /// once-event that fired (byte 141, which a Yes rewrites), the class.
    fn eligible(&self, id: EventId, w: &dyn EventWorld) -> Result<(), String> {
        let e = self.ev(id);
        let now = w.now();
        if e.subordinate != 0 {
            return Err("done: fires only through a chain".into());
        }
        if !self.may_refire(id, now) {
            return Err("firing guard".into());
        }
        if !self.is_open(id, now) {
            let start = self.start_time(id).unwrap_or(0);
            return Err(format!("window closed (start {start}, now {now})"));
        }
        if e.once != 0 && self.st(id).times != 0 {
            return Err("done (once, fired)".into());
        }
        if e.archetype != 0 && e.archetype != w.hero_archetype() {
            return Err("class".into());
        }
        Ok(())
    }

    /// The full check of an event with the current meeting.
    fn passes(&self, id: EventId, w: &dyn EventWorld) -> bool {
        self.eligible(id, w).is_ok() && self.failing_with(id, w, self.meeting).is_none()
    }

    /// The events the scan checks where the player stands, in the original's order
    /// (0x4abfbc): the global events in file order; then the event point's listed events
    /// whatever their kind; or the building's local events (in villages and shipyards every
    /// listed event), only on entering it.
    fn candidates(&self, place: Option<Place>) -> Vec<EventId> {
        let mut ids: Vec<EventId> =
            (1..=self.events.len() as EventId).filter(|&id| self.ev(id).kind() == Some(EventKind::Global)).collect();
        let here = match place {
            Some(Place::Building(_)) if !self.fresh_visit => None,
            p => p.and_then(|p| self.places.get(&p)),
        };
        let any_kind = place.is_some_and(|p| matches!(p, Place::Point(_)) || self.every_kind.contains(&p));
        for &id in here.into_iter().flatten() {
            if any_kind || self.ev(id).kind() == Some(EventKind::Local) {
                ids.push(id);
            }
        }
        ids
    }

    /// Why event `id` does not fire now where the player stands (a debugging aid): out of
    /// scope, not eligible, or the first failing condition. `None`: it would fire.
    pub fn why_not(&self, id: EventId, w: &dyn EventWorld) -> Option<String> {
        let e = self.event(id)?;
        if !self.candidates(w.place()).contains(&id) {
            let why = match e.kind() {
                _ if e.subordinate != 0 => "subordinate: fires only through a chain",
                Some(EventKind::Quest | EventKind::Rumour) => "quest or rumour: taken in the main hall",
                Some(EventKind::Global) => "global event out of scope",
                _ => "not here (a local event of another place, or the building was not just entered)",
            };
            return Some(why.into());
        }
        if let Err(why) = self.eligible(id, w) {
            return Some(why);
        }
        self.failing_condition(id, w).map(|c| format!("condition: {c}"))
    }

    /// Army `army` is marked "meeting event waiting" (0x4a801a, recomputed by 0x4abf44):
    /// some event that needs a meeting with it is eligible and every condition checked
    /// before the meet-army test holds.
    pub fn meeting_waiting(&self, w: &dyn EventWorld, army: ArmyId) -> bool {
        let waits = |id: EventId| {
            self.ev(id).conditions.meet_army == army
                && self.eligible(id, w).is_ok()
                && self.check(id, w, Some(army)).is_none_or(|f| f.after_meeting)
        };
        army != 0 && (1..=self.events.len() as EventId).any(waits)
    }

    /// The first event that passes the full check.
    fn first_eligible(&self, w: &dyn EventWorld) -> Option<EventId> {
        self.candidates(w.place()).into_iter().find(|&id| self.passes(id, w))
    }

    // ---------------------------------------------------------------------------------------
    // Opening and finishing
    // ---------------------------------------------------------------------------------------

    /// Open an event (0x4a8ae8): with "ask" set its question waits; else the dialog-time
    /// results are applied and it is finished (the original finishes at OK, or at once for a
    /// silent event; nothing changes in between).
    fn open(&mut self, id: EventId, w: &mut dyn EventWorld, out: &mut Vec<EventOutcome>, depth: usize) {
        if self.ev(id).conditions.confirm_question != 0 {
            self.pending = Some(id);
            out.push(EventOutcome::Question(id));
        } else {
            self.show(id, w, out);
            self.finish(id, w, out, depth);
        }
    }

    /// The window without the question (0x4a8ae8, ask clear): the answer is cleared and the
    /// dialog-time results applied in the original's order: artifacts gained, artifacts lost,
    /// units added, units removed, spells learned.
    fn show(&mut self, id: EventId, w: &mut dyn EventWorld, out: &mut Vec<EventOutcome>) {
        self.st_mut(id).set_answer_byte(0);
        let e = self.ev(id).clone();
        out.push(EventOutcome::Fired { event: id, message: !e.message.is_empty() });
        // A message opens the window (0x4ac3b4) and the scan stops there; an event without
        // one is finished at once and the scan goes on.
        if !e.message.is_empty() {
            self.held = true;
        }
        let r = &e.results;
        // Opcode 6 gives the four artifacts to a unit instead (c278e8).
        if opcode(&e) != Some(6) {
            for a in nonzero(&r.artifacts_add) {
                w.give_item(a);
            }
        }
        // Artifacts lost: only when at least one of them is held (0x4a8fe5).
        let (pack, worn) = w.player_items();
        if nonzero(&r.artifacts_remove).any(|a| pack.contains(&a) || worn.contains(&a)) {
            for a in nonzero(&r.artifacts_remove) {
                w.take_item(a);
            }
        }
        let from = (r.units_from_army != 0).then_some(r.units_from_army);
        for (i, u) in r.units_add.iter().enumerate() {
            if *u != 0 {
                w.add_unit(*u, r.units_add_named[i], from);
            }
        }
        // A source army left empty is deactivated (0x496900), which ends a meeting with it.
        if let Some(a) = from {
            if self.meeting == Some(a) && w.army_inactive(a) {
                self.meeting = None;
            }
        }
        // Units removed: nothing happens unless one slot finds a unit. The search would
        // reach the hero by a type match; Razdor never removes him *(guess: whether the
        // original could, events.md Unknowns)*.
        let to = (r.removed_units_to_army != 0).then_some(r.removed_units_to_army);
        let slots: Vec<(u8, u8)> = (0..4).filter(|&i| r.units_remove[i] != 0).map(|i| (r.units_remove[i], r.units_remove_named[i])).collect();
        let units = w.player_units();
        if slots.iter().any(|&(t, n)| find_unit(&units, t, n, 1).is_some()) {
            for (t, n) in slots {
                if let Some(i) = find_unit(&w.player_units(), t, n, 1) {
                    w.remove_unit(i, n != 0, to);
                }
            }
        }
        // The Community hook (0xc28604) skips the learning whenever the event's "no meeting"
        // byte is 1, opcode or not.
        if r.no_meeting != 1 {
            for s in nonzero(&r.spells_learned) {
                w.learn_spell(s);
            }
        }
    }

    /// Deactivate `army` (0x496900): it also stops being the army met, so the meet-army
    /// events of the rest of the run fail for it. Event 6 of the first tutorial map asks the
    /// ghost (army 4) and a Yes sends it away; event 7, "meet army 4 and Yes to 6", waits for
    /// the next meeting, after event 8 has brought it back.
    fn deactivate(&mut self, w: &mut dyn EventWorld, army: ArmyId) {
        w.deactivate_army(army);
        if self.meeting == Some(army) {
            self.meeting = None;
        }
    }

    /// Finish an event (0x4ab1ec → 0x4ab2a3 → 0x4ab51b), then its follow-ups.
    fn finish(&mut self, id: EventId, w: &mut dyn EventWorld, out: &mut Vec<EventOutcome>, depth: usize) {
        let now = w.now();
        let e = self.ev(id).clone();
        let r = &e.results;
        let op = opcode(&e);
        // 1. "No meeting": the meeting ends here, for the rest of the run too.
        if r.no_meeting != 0 {
            self.meeting = None;
        }
        // 2. The guard and the count; opcode 18 draws the digit of this event's `^` flag.
        let st = self.st_mut(id);
        st.last_fired = Some(now + 1);
        st.times += 1;
        if op == Some(18) {
            // Raw bytes: the low bytes of the XP and gold fields, read signed.
            let (lo, hi) = (r.experience as i8 as i64, r.gold as i8 as i64);
            self.random_digit = Some(w.random(lo, hi) as u8);
        }
        // 3. The flag action, and the tutorial's end mark.
        if let Some(f) = &e.flags {
            self.flag_action(f.action());
            if f.test().contains("end_tutorial") {
                self.tutorial_done = true;
            }
        }
        // 4–6. XP, gold, mana; in opcode mode the opcode instead (their fields are its
        // arguments).
        match op {
            Some(op) => self.run_opcode(id, op, &e, w),
            None => {
                if r.experience != 0 {
                    w.add_experience(r.experience as i64);
                }
                if r.gold != 0 {
                    w.add_gold(r.gold as i64);
                }
                if r.mana != 0 {
                    w.add_mana(r.mana as i64);
                }
            }
        }
        // 7. Activate, then deactivate.
        for a in nonzero(&r.activate_armies) {
            w.activate_army(a);
        }
        if r.deactivate_army != 0 {
            self.deactivate(w, r.deactivate_army);
        }
        // 8. The patrol; in opcode mode the opcode is the delta all the same (the patch
        // reuses the byte and leaves this step alone).
        if r.patrol_army != 0 && r.patrol_delta != 0 {
            w.change_patrol(r.patrol_army, r.patrol_delta);
        }
        // 9–10. The shown army and the lanterns.
        let mut q = Queued::default();
        if r.show_army != 0 {
            w.show_army(r.show_army);
            q.shown = true;
        }
        for p in nonzero(&r.light_lanterns) {
            w.light_lantern(p);
            q.shown = true;
        }
        if q.shown {
            w.shown_by(id);
        }
        // 11. A quest adds itself to the journal, every time it finishes (0x49c170).
        if e.kind() == Some(EventKind::Quest) {
            self.journal.push(id);
            out.push(EventOutcome::QuestAdded(id));
        }
        // 12. Completing a quest removes its last journal entry (0x49c2b8).
        let done = r.completes_quest;
        if self.event(done).is_some() {
            if let Some(k) = self.journal.iter().rposition(|&j| j == done) {
                self.journal.remove(k);
            }
            if !self.completed.contains(&done) {
                self.completed.push(done);
            }
            out.push(EventOutcome::QuestCompleted(done));
        }
        // 13–17. Queued: the spell, the battle, the delay; the army moved to the hero (byte
        // 136, not 142) and the new class at once.
        q.spell = r.cast_spell;
        if r.move_to_hero != 0 && r.removed_units_to_army != 0 {
            w.move_army_to_hero(r.removed_units_to_army);
        }
        q.battle = r.start_battle_with;
        if r.new_hero_class != 0 {
            w.set_hero_class(r.new_hero_class);
        }
        q.delay = r.delay_hours;
        // 19. The relative event's start is re-based.
        let rel = r.relative_event;
        if self.event(rel).is_some() {
            self.st_mut(rel).start = Some(now + r.relative_delay_hours as u64 * 60);
        }
        // 20. Victory or defeat: processing stops, no follow-up and no chain.
        if id == self.victory || id == self.defeat {
            let end = if id == self.victory { EventOutcome::Victory(id) } else { EventOutcome::Defeat(id) };
            out.push(end.clone());
            self.ended = Some(end);
            return;
        }
        // 21. The follow-ups. The answer is cleared.
        self.st_mut(id).set_answer_byte(0);
        if q.battle != 0 {
            w.start_battle(q.battle);
        }
        if q.spell != 0 {
            w.apply_spell(q.spell);
        }
        if q.delay != 0 {
            w.delay_player(q.delay as u64 * 60);
        }
        // A chain behind a delay is dropped: the delay path starts no chain timer (a battle
        // does not count; lanterns, a shown army or a spell start one).
        if q.delay != 0 && !q.shown && q.spell == 0 {
            return;
        }
        let next = r.chained_event;
        if self.held && self.event(next).is_some() {
            // Its window is up: the chain waits for it to be closed.
            self.held_chain = Some(next);
        } else if depth < CHAIN_DEPTH && self.event(next).is_some() {
            // A chained event is opened as it is: its done flag, window, guard, once flag,
            // class, place and conditions are not checked (0x4ab1ec, 0x4af658).
            self.open(next, w, out, depth + 1);
        }
    }

    /// The effect of a Community opcode (events.md §15; arguments: XP `x`, gold `g`, mana
    /// `m`).
    fn run_opcode(&mut self, id: EventId, op: i8, e: &Event, w: &mut dyn EventWorld) {
        let r = &e.results;
        let (x, g, m) = (r.experience, r.gold, r.mana);
        let holder = Holder::from_code(x);
        // −1 (any negative) means the whole army; slots past 255 do not exist.
        let units = |v: i16| if v < 0 { None } else { Some(v.min(255) as u8) };
        let unit = u8::try_from(g).ok();
        let spells: Vec<u8> = nonzero(&r.spells_learned).collect();
        if !(6..=22).contains(&op) {
            // The byte pokes (c2669e, c26826): opcodes 1, 2 and the negative ones poke with
            // the first setting, every opcode outside 6–22 with the second when its action
            // is 1, 2 or negative; 2 sets the byte, the others add to it (wrapping).
            let [first, second] = edit_settings(e);
            for ed in [first].into_iter().filter(|_| op <= 2).chain([second]) {
                if ed.action > 2 || ed.action == 0 {
                    continue;
                }
                let at = (id as i64 - 1 + ed.shift as i64) * RECORD + ed.field as i64;
                let Some((target, off)) = self.byte_at(at) else { continue };
                let Some(old) = self.record_byte(target, off) else { continue };
                let v = ed.value as u8;
                self.set_record_byte(target, off, if ed.action == 2 { v } else { old.wrapping_add(v) });
            }
            return;
        }
        match op {
            6 => {
                if let (Some(h), Some(u)) = (holder, unit) {
                    w.equip_unit(h, u, r.artifacts_add);
                }
            }
            7 => {
                if let (Some(h), Some(u), Ok(with)) = (holder, unit, u8::try_from(m)) {
                    w.replace_unit(h, u, with);
                }
            }
            8 => {
                if let Some(h) = holder {
                    w.set_army_speed(h, speed_correction(g));
                }
            }
            9 => {
                if let (Some(h), Some(group)) = (holder, unit.filter(|u| (1..=4).contains(u))) {
                    w.set_faction(h, group);
                }
            }
            10 => {
                if let (Some(h), Some(group)) = (holder, unit.filter(|u| *u <= 3)) {
                    w.set_relation(h, group, m.clamp(-3, 3) as i8);
                }
            }
            11 => {
                // Slot k takes entry k; a 0 empties its slot.
                if let Some(h) = holder {
                    w.set_spells(h, units(g), &r.spells_learned);
                }
            }
            12 => {
                if let (Some(h), Some(u), Ok(named)) = (holder, unit, u8::try_from(m)) {
                    let class = (named as usize).checked_sub(1).and_then(|k| self.named_units.get(k)).copied().unwrap_or(0);
                    w.set_named_unit(h, u, named, class);
                }
            }
            13 => {
                if let Some(h) = holder {
                    w.give_unit_xp(h, units(g), m as i64);
                }
            }
            15 => self.branch = Some((x, g)),
            16 => {
                for s in spells {
                    w.forget_spell(s);
                }
            }
            17 => {
                if let (Some(h @ (Holder::Player | Holder::Army(_))), Some(model)) = (holder, unit) {
                    w.set_army_model(h, model);
                }
            }
            19 => {
                if let Ok(army @ 1..=255) = u8::try_from(x) {
                    w.set_army_target(army, g as i32, m as i32);
                }
            }
            20 => w.teleport_player(x as i32, g as i32),
            // 14 is a condition, 18 draws its digit at the finish, 21 is a condition, 22
            // does nothing.
            _ => {}
        }
    }

    /// A byte compare of opcodes 3–5 (c26754, c268b4): 3 needs the byte below the value, 4
    /// equal, 5 above, as signed bytes. A byte Razdor does not model passes *(guess)*.
    fn byte_compare(&self, id: EventId, ed: &EventEdit) -> bool {
        let at = (id as i64 - 1 + ed.shift as i64) * RECORD + ed.field as i64;
        let Some(b) = self.byte_at(at).and_then(|(t, off)| self.record_byte(t, off)) else { return true };
        let (b, v) = (b as i8, ed.value as i8);
        match ed.action {
            3 => b < v,
            4 => b == v,
            5 => b > v,
            _ => true,
        }
    }

    // ---------------------------------------------------------------------------------------
    // Conditions
    // ---------------------------------------------------------------------------------------

    /// The first condition of event `id` that does not hold (eligibility, the place and the
    /// question aside), named; `None` when they all hold.
    pub fn failing_condition(&self, id: EventId, w: &dyn EventWorld) -> Option<&'static str> {
        self.failing_with(id, w, self.meeting)
    }

    /// [`EventEngine::failing_condition`] with `meeting` as the army being met.
    fn failing_with(&self, id: EventId, w: &dyn EventWorld, meeting: Option<ArmyId>) -> Option<&'static str> {
        self.check(id, w, meeting).map(|f| f.name)
    }

    /// The conditions in the original's order (0x4a7b80 and its continuations).
    fn check(&self, id: EventId, w: &dyn EventWorld, meeting: Option<ArmyId>) -> Option<Failed> {
        let e = self.ev(id);
        let c = &e.conditions;
        let r = &e.results;
        let op = opcode(e);
        let before = |name| Some(Failed { name, after_meeting: false });
        let after = |name| Some(Failed { name, after_meeting: true });
        if c.defeated_check != 0 && !nonzero(&c.defeated_armies).all(|a| w.player_defeated(a)) {
            return before("defeated by the player");
        }
        // The lists name events; times fired and the answer byte are read (an answer of 1 is
        // "the last response was No").
        let times = |id: EventId| self.event(id).map_or(0, |_| self.st(id).times);
        // "With Yes" fails on an answer of 1, "with No" on an answer of 0: a byte poked to
        // another value passes both, as in the original.
        let answer = |id: EventId| self.event(id).map_or(0, |_| self.st(id).answer_byte());
        if c.happened_yes_check != 0 && !nonzero(&c.happened_yes).all(|id| times(id) > 0 && answer(id) != 1) {
            return before("happened with Yes");
        }
        if c.happened_no_check != 0 && !nonzero(&c.happened_no).all(|id| answer(id) != 0) {
            return before("happened with No");
        }
        if c.not_happened_check != 0 && !nonzero(&c.not_happened).all(|id| times(id) == 0) {
            return before("not happened");
        }
        if c.army_active != 0 && !w.army_active(c.army_active) {
            return before("army active");
        }
        if c.army_inactive != 0 && !w.army_inactive(c.army_inactive) {
            return before("army inactive");
        }
        if c.beaten_check != 0 && !nonzero(&c.beaten_armies).all(|a| w.army_beaten(a)) {
            return before("beaten by anyone");
        }
        if c.meet_army != 0 && meeting != Some(c.meet_army) {
            return before("meet army");
        }
        // Byte 145 is a condition: the hero's HP is exactly 1.
        if r.hero_one_hp != 0 && w.hero_hp() != 1 {
            return after("hero at 1 HP");
        }
        if c.army_at_home != 0 && !w.army_at_home(c.army_at_home) {
            return after("army at home");
        }
        // Opcode 14 (c28048).
        if op == Some(14) {
            let spells: Vec<u8> = nonzero(&r.spells_learned).collect();
            let units = if r.gold < 0 { None } else { u8::try_from(r.gold).ok() };
            if !Holder::from_code(r.experience).is_some_and(|h| w.has_spells(h, units, &spells)) {
                return after("opcode 14");
            }
        }
        // The numeric conditions only with "current stats", and (Community, c2698c) not when
        // the patrol value is set and the squad value is positive: that also skips plain
        // events that change a patrol and ask for a squad count.
        if c.stats_check != 0 && (r.patrol_delta == 0 || c.squad_count <= 0) {
            if !compare(w.hero_level(), c.level) {
                return after("level");
            }
            // The cell test of opcode 19 (c2875e) replaces the rest whenever "no meeting" is 1,
            // the patrol value is at most 19 (signed: 0 and below too) and the strength's low
            // byte names an army.
            let army = c.army_strength as u8;
            if r.no_meeting == 1 && r.patrol_delta <= 19 && army != 0 {
                if !w.army_at(army, c.gold as i32, c.holiness_mana as i32) {
                    return after("army at the cell");
                }
            } else {
                if !compare(w.gold(), c.gold) || !compare(w.mana(), c.holiness_mana) {
                    return after("gold / mana");
                }
                if !compare(w.squad_count(), c.squad_count) || (c.army_strength != 0 && !compare(w.army_strength(), c.army_strength)) {
                    return after("squad count / army strength");
                }
            }
        }
        // Opcodes 3–5 compare bytes of event records (c26754), and a second compare driven
        // by the squad value's low byte runs for any opcode (c268b4).
        if let Some(op) = op {
            let [first, second] = edit_settings(e);
            if (3..=5).contains(&op) && !self.byte_compare(id, &first) {
                return after("opcode compare");
            }
            if (3..=5).contains(&second.action) && !self.byte_compare(id, &second) {
                return after("opcode compare");
            }
        }
        // Building owners (0x4a815a): a slot with both an id and a code; code 1 the player's,
        // 6 not his, and for every code the faction test (code − 1): code 1 also passes on
        // faction 0 and code 6 on faction 5, as in the original.
        if c.buildings_check != 0 {
            for i in 0..3 {
                let (b, code) = (c.buildings[i], c.buildings_owner[i]);
                if b == 0 || code == 0 {
                    continue;
                }
                let Some((mine, faction)) = w.building_state(b as u16) else { return after("buildings") };
                let ok = (code == SIDE_PLAYER && mine) || (code == OWNER_NOT_PLAYER && !mine) || code as i32 - 1 == faction as i32;
                if !ok {
                    return after("buildings");
                }
            }
        }
        if op == Some(21) {
            if !self.op21_holds(e, w) {
                return after("named units");
            }
        } else if c.units_check != 0 && !self.units_hold(c, w) {
            return after("named units");
        }
        if c.artifacts_check != 0 && !self.artifacts_hold(c, w) {
            return after("artifacts");
        }
        if let Some(f) = &e.flags {
            if !self.flag_test(f.test()) {
                return after("flag test");
            }
        }
        None
    }

    /// The named-squad condition (0x4a8237). Each slot looks for its own unit. Codes 1 and 6
    /// search the hero's army for a living unit no earlier slot took: of the slot's type when
    /// the slot has no name (the unit's own name is not looked at, so a named character of
    /// that type counts too), or carrying the slot's name, or for type 255 an unnamed event
    /// unit; code 1 needs one, 6 none. Codes 2–5 search the armies of faction code − 1
    /// alike, without the alive test and the 255 rule. Code 0 fails.
    ///
    /// The units taken go into a list of three places, as in the original: once it is full,
    /// each new one overwrites the third, so the unit noted there is free again.
    fn units_hold(&self, c: &crate::dt::dtm::EventConditions, w: &dyn EventWorld) -> bool {
        let fits = |u: &UnitRecord, t: u8, name: u8, player: bool| {
            (u.unit == t as u32 && name == 0)
                || (u.named > 0 && u.named == name)
                || (player && t == 0xFF && u.from_event && u.named == 0)
        };
        let mine = w.player_units();
        let mut used: [Option<(usize, usize)>; 3] = [None; 3];
        let note = |used: &mut [Option<(usize, usize)>; 3], at: (usize, usize)| {
            let k = used.iter().position(Option::is_none).unwrap_or(2);
            used[k] = Some(at);
        };
        for i in 0..3 {
            let (t, name, code) = (c.units[i], c.units_named[i], c.units_owner[i]);
            if t == 0 {
                continue;
            }
            let ok = match code {
                SIDE_PLAYER | OWNER_NOT_PLAYER => {
                    let found = (0..mine.len()).find(|&k| !used.contains(&Some((0, k))) && mine[k].hp != 0 && fits(&mine[k], t, name, true));
                    if let Some(k) = found {
                        note(&mut used, (0, k));
                    }
                    (code == SIDE_PLAYER) == found.is_some()
                }
                2..=5 => {
                    let mut any = false;
                    for (a, army) in w.faction_units(code - 1).iter().enumerate() {
                        if let Some(k) = (0..army.len()).find(|&k| !used.contains(&Some((a + 1, k))) && fits(&army[k], t, name, false)) {
                            note(&mut used, (a + 1, k));
                            any = true;
                        }
                    }
                    any
                }
                _ => false,
            };
            if !ok {
                return false;
            }
        }
        true
    }

    /// Opcode 21's named-squad check (c2a9a2): for each slot whose name a unit of the hero's
    /// army carries, that unit must be of the slot's type, compared with the byte as stored
    /// (without the original's −1 shift: one type off). A slot counts when the four bytes
    /// from its name on are not all 0 (a dword test: the next names and a code too).
    fn op21_holds(&self, e: &Event, w: &dyn EventWorld) -> bool {
        let c = &e.conditions;
        let bytes: Vec<u8> = c.units_named.iter().chain(c.units_owner.iter()).copied().collect();
        let units = w.player_units();
        (0..3).all(|j| {
            if bytes[j..j + 4].iter().all(|&b| b == 0) {
                return true;
            }
            match units.iter().find(|u| u.named == c.units_named[j]) {
                Some(u) => u.unit.wrapping_sub(1) == c.units[j] as u32,
                None => true,
            }
        })
    }

    /// The artifact condition (0x4a815a tail). Each slot looks in the pack, then on the hero's
    /// units; a found item is taken aside for the rest of the check, so two slots with the
    /// same artifact need two copies. Code 1 passes when found, 6 when not; a slot that has
    /// not passed then searches the worn items of the armies of faction code − 1 (their
    /// packs are not). Code 0 fails.
    fn artifacts_hold(&self, c: &crate::dt::dtm::EventConditions, w: &dyn EventWorld) -> bool {
        let (mut pack, mut worn) = w.player_items();
        let mut ai: HashMap<u8, Vec<u8>> = HashMap::new();
        let take = |list: &mut Vec<u8>, a: u8| list.iter().position(|&x| x == a).map(|k| list.remove(k)).is_some();
        for i in 0..3 {
            let (a, code) = (c.artifacts[i], c.artifacts_owner[i]);
            if a == 0 {
                continue;
            }
            let found = pack.contains(&a) || worn.contains(&a);
            let mut ok = (code == SIDE_PLAYER && found) || (code == OWNER_NOT_PLAYER && !found);
            // Only an item that passed its slot stays aside.
            if ok && found && !take(&mut pack, a) {
                take(&mut worn, a);
            }
            if !ok && code != 0 {
                let faction = code - 1;
                let items = ai.entry(faction).or_insert_with(|| w.faction_worn_items(faction));
                ok = take(items, a);
            }
            if !ok {
                return false;
            }
        }
        true
    }
}

#[cfg(test)]
pub(crate) mod mock {
    //! A world of plain fields for the engine tests.
    use super::*;
    use std::collections::{HashMap, HashSet};

    /// An effect the engine asked for, in order.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub enum Fx {
        Xp(i64),
        Gold(i64),
        Mana(i64),
        AddUnit(u8, u8, Option<ArmyId>),
        RemoveUnit(usize, Option<ArmyId>),
        GiveItem(u8),
        TakeItem(u8),
        Learn(u8),
        Spell(u8),
        Activate(ArmyId),
        Deactivate(ArmyId),
        Show(ArmyId),
        MoveToHero(ArmyId),
        Lantern(u16),
        Patrol(ArmyId, i8),
        Class(u8),
        Battle(ArmyId),
        Delay(u64),
        Equip(Holder, u8, [u8; 4]),
        Replace(Holder, u8, u8),
        Speed(Holder, i8),
        Faction(Holder, u8),
        Relation(Holder, u8, i8),
        SetSpells(Holder, Option<u8>, Vec<u8>),
        Named(Holder, u8, u8, u8),
        UnitXp(Holder, Option<u8>, i64),
        Forget(u8),
        Model(Holder, u8),
        Target(ArmyId, i32, i32),
        Teleport(i32, i32),
    }

    #[derive(Clone, Debug, Default)]
    pub struct MockWorld {
        pub now: u64,
        pub archetype: u8,
        pub level: i64,
        pub hp: i64,
        pub gold: i64,
        pub mana: i64,
        pub squads: i64,
        pub strength: i64,
        pub building_owner: HashMap<u16, u8>,
        /// The player's units, the hero first.
        pub units: Vec<UnitRecord>,
        /// Armies by faction, each with its units.
        pub armies: HashMap<u8, Vec<Vec<UnitRecord>>>,
        pub pack: Vec<u8>,
        pub worn: Vec<u8>,
        /// What the armies of a faction wear.
        pub ai_worn: HashMap<u8, Vec<u8>>,
        pub defeated: HashSet<ArmyId>,
        pub beaten: HashSet<ArmyId>,
        pub active: HashSet<ArmyId>,
        pub inactive: HashSet<ArmyId>,
        pub home: HashSet<ArmyId>,
        pub place: Option<Place>,
        /// Lasting spells by holder (opcodes 11 and 14).
        pub spells_on: HashMap<Holder, Vec<u8>>,
        /// Army cells (opcode 19).
        pub army_cells: HashMap<ArmyId, (i32, i32)>,
        /// What `random` returns, in turn (else the low end).
        pub rolls: Vec<i64>,
        pub log: Vec<Fx>,
    }

    /// A unit of `unit`, alive.
    pub fn unit(unit: u32, named: u8) -> UnitRecord {
        UnitRecord { unit, named, hp: 10, from_event: false }
    }

    impl MockWorld {
        /// A world with the hero alone, at 10 HP.
        pub fn new() -> MockWorld {
            MockWorld { archetype: 1, level: 1, hp: 10, units: vec![unit(1, 0)], ..MockWorld::default() }
        }
    }

    impl EventWorld for MockWorld {
        fn now(&self) -> u64 {
            self.now
        }
        fn hero_archetype(&self) -> u8 {
            self.archetype
        }
        fn hero_level(&self) -> i64 {
            self.level
        }
        fn hero_hp(&self) -> i64 {
            self.hp
        }
        fn gold(&self) -> i64 {
            self.gold
        }
        fn mana(&self) -> i64 {
            self.mana
        }
        fn squad_count(&self) -> i64 {
            self.squads
        }
        fn army_strength(&self) -> i64 {
            self.strength
        }
        /// Side codes: 1 the player's (faction 1), 2–5 a faction's (code − 1); none: a
        /// neutral building of faction 3.
        fn building_state(&self, building: u16) -> Option<(bool, u8)> {
            Some(match self.building_owner.get(&building).copied() {
                Some(SIDE_PLAYER) => (true, 1),
                Some(code) => (false, code - 1),
                None => (false, 3),
            })
        }
        fn player_units(&self) -> Vec<UnitRecord> {
            self.units.clone()
        }
        fn faction_units(&self, faction: u8) -> Vec<Vec<UnitRecord>> {
            self.armies.get(&faction).cloned().unwrap_or_default()
        }
        fn player_items(&self) -> (Vec<u8>, Vec<u8>) {
            (self.pack.clone(), self.worn.clone())
        }
        fn faction_worn_items(&self, faction: u8) -> Vec<u8> {
            self.ai_worn.get(&faction).cloned().unwrap_or_default()
        }
        fn player_defeated(&self, army: ArmyId) -> bool {
            self.defeated.contains(&army)
        }
        fn army_beaten(&self, army: ArmyId) -> bool {
            self.beaten.contains(&army)
        }
        fn army_active(&self, army: ArmyId) -> bool {
            self.active.contains(&army)
        }
        fn army_inactive(&self, army: ArmyId) -> bool {
            self.inactive.contains(&army)
        }
        fn army_at_home(&self, army: ArmyId) -> bool {
            self.home.contains(&army)
        }
        fn place(&self) -> Option<Place> {
            self.place
        }
        fn add_experience(&mut self, xp: i64) {
            self.log.push(Fx::Xp(xp));
        }
        fn add_gold(&mut self, gold: i64) {
            self.gold = (self.gold + gold).max(0);
            self.log.push(Fx::Gold(gold));
        }
        fn add_mana(&mut self, mana: i64) {
            self.mana += mana;
            self.log.push(Fx::Mana(mana));
        }
        fn add_unit(&mut self, u: u8, named: u8, from_army: Option<ArmyId>) {
            self.squads += 1;
            self.units.push(UnitRecord { from_event: true, ..unit(u as u32, named) });
            self.log.push(Fx::AddUnit(u, named, from_army));
        }
        fn remove_unit(&mut self, index: usize, _lead: bool, to_army: Option<ArmyId>) {
            self.squads -= 1;
            self.units.remove(index);
            self.log.push(Fx::RemoveUnit(index, to_army));
        }
        fn give_item(&mut self, artifact: u8) {
            self.pack.push(artifact);
            self.log.push(Fx::GiveItem(artifact));
        }
        fn take_item(&mut self, artifact: u8) {
            if let Some(k) = self.pack.iter().position(|&a| a == artifact) {
                self.pack.remove(k);
            }
            self.log.push(Fx::TakeItem(artifact));
        }
        fn learn_spell(&mut self, spell: u8) {
            self.log.push(Fx::Learn(spell));
        }
        fn apply_spell(&mut self, spell: u8) {
            self.log.push(Fx::Spell(spell));
        }
        fn activate_army(&mut self, army: ArmyId) {
            self.active.insert(army);
            self.log.push(Fx::Activate(army));
        }
        fn deactivate_army(&mut self, army: ArmyId) {
            self.active.remove(&army);
            self.log.push(Fx::Deactivate(army));
        }
        fn show_army(&mut self, army: ArmyId) {
            self.log.push(Fx::Show(army));
        }
        fn move_army_to_hero(&mut self, army: ArmyId) {
            self.log.push(Fx::MoveToHero(army));
        }
        fn light_lantern(&mut self, point: u16) {
            self.log.push(Fx::Lantern(point));
        }
        fn change_patrol(&mut self, army: ArmyId, delta: i8) {
            self.log.push(Fx::Patrol(army, delta));
        }
        fn set_hero_class(&mut self, unit: u8) {
            self.log.push(Fx::Class(unit));
        }
        fn start_battle(&mut self, army: ArmyId) {
            self.log.push(Fx::Battle(army));
        }
        fn delay_player(&mut self, minutes: u64) {
            self.now += minutes;
            self.log.push(Fx::Delay(minutes));
        }
        fn equip_unit(&mut self, holder: Holder, unit: u8, items: [u8; 4]) {
            self.log.push(Fx::Equip(holder, unit, items));
        }
        fn replace_unit(&mut self, holder: Holder, unit: u8, with: u8) {
            self.log.push(Fx::Replace(holder, unit, with));
        }
        fn set_army_speed(&mut self, holder: Holder, correction: i8) {
            self.log.push(Fx::Speed(holder, correction));
        }
        fn set_faction(&mut self, holder: Holder, group: u8) {
            self.log.push(Fx::Faction(holder, group));
        }
        fn set_relation(&mut self, holder: Holder, group: u8, value: i8) {
            self.log.push(Fx::Relation(holder, group, value));
        }
        fn set_spells(&mut self, holder: Holder, unit: Option<u8>, spells: &[u8]) {
            self.spells_on.insert(holder, spells.to_vec());
            self.log.push(Fx::SetSpells(holder, unit, spells.to_vec()));
        }
        fn set_named_unit(&mut self, holder: Holder, unit: u8, named: u8, class: u8) {
            self.log.push(Fx::Named(holder, unit, named, class));
        }
        fn give_unit_xp(&mut self, holder: Holder, unit: Option<u8>, xp: i64) {
            self.log.push(Fx::UnitXp(holder, unit, xp));
        }
        fn has_spells(&self, holder: Holder, _unit: Option<u8>, spells: &[u8]) -> bool {
            let on = self.spells_on.get(&holder).cloned().unwrap_or_default();
            spells.iter().all(|s| on.contains(s))
        }
        fn forget_spell(&mut self, spell: u8) {
            self.log.push(Fx::Forget(spell));
        }
        fn set_army_model(&mut self, holder: Holder, model: u8) {
            self.log.push(Fx::Model(holder, model));
        }
        fn random(&mut self, lo: i64, hi: i64) -> i64 {
            if self.rolls.is_empty() {
                lo
            } else {
                self.rolls.remove(0).clamp(lo, hi)
            }
        }
        fn set_army_target(&mut self, army: ArmyId, x: i32, y: i32) {
            self.log.push(Fx::Target(army, x, y));
        }
        fn army_at(&self, army: ArmyId, x: i32, y: i32) -> bool {
            self.army_cells.get(&army) == Some(&(x, y))
        }
        fn teleport_player(&mut self, x: i32, y: i32) {
            self.log.push(Fx::Teleport(x, y));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::mock::{unit, Fx, MockWorld};
    use super::*;
    use crate::dt::dtm::FlagScript;

    const DAY: u64 = 1440;

    /// A once-event of `kind` that is open all the time from minute 0 (daily windows).
    fn ev(kind: EventKind) -> Event {
        Event { kind: kind as u8, repeat: DAY as u16, duration: DAY as u16, once: 1, ..Event::default() }
    }

    fn global() -> Event {
        ev(EventKind::Global)
    }

    fn many(mut e: Event) -> Event {
        e.once = 0;
        e
    }

    fn titled(mut e: Event, script: &str) -> Event {
        e.title = format!("t%{script}");
        e.flags = FlagScript::from_title(&e.title);
        e
    }

    fn with_message(mut e: Event) -> Event {
        e.message = "m".into();
        e
    }

    fn asking(mut e: Event) -> Event {
        e.conditions.confirm_question = 1;
        e.question = "q".into();
        e
    }

    fn engine(events: Vec<Event>) -> EventEngine {
        EventEngine::from_parts(events, Vec::new(), 0, 0)
    }

    fn fired(out: &[EventOutcome]) -> Vec<EventId> {
        out.iter()
            .filter_map(|o| match o {
                EventOutcome::Fired { event, .. } => Some(*event),
                _ => None,
            })
            .collect()
    }

    fn tick_at(g: &mut EventEngine, w: &mut MockWorld, now: u64) -> Vec<EventId> {
        w.now = now;
        let out = g.tick(w);
        fired(&read(g, w, out))
    }

    /// A meeting, every window it opens read.
    fn meet_read(g: &mut EventEngine, w: &mut MockWorld, army: ArmyId) -> Vec<EventOutcome> {
        let out = g.meet(w, army);
        read(g, w, out)
    }

    /// The player reads every window that opens (OK), the scan going on after each.
    fn read(g: &mut EventEngine, w: &mut MockWorld, mut out: Vec<EventOutcome>) -> Vec<EventOutcome> {
        while g.holds_window() {
            out.extend(g.window_closed(w));
        }
        out
    }

    #[test]
    fn time_window_and_repeat() {
        // Daily from minute 1000, open for 1 hour: a many-event fires on every later scan.
        let mut e = many(global());
        (e.start_time, e.repeat, e.duration) = (1000, DAY as u16, 1);
        let mut g = engine(vec![e]);
        let mut w = MockWorld::new();
        assert!(tick_at(&mut g, &mut w, 999).is_empty(), "before the start");
        assert_eq!(tick_at(&mut g, &mut w, 1000), vec![1], "once in a scan");
        assert_eq!(tick_at(&mut g, &mut w, 1000), vec![1], "the idle reset opens it again, even in the same minute");
        assert_eq!(tick_at(&mut g, &mut w, 1030), vec![1]);
        assert!(tick_at(&mut g, &mut w, 1061).is_empty(), "the hour is over");
        assert_eq!(tick_at(&mut g, &mut w, 1000 + DAY + 59), vec![1], "next day's window");
        assert_eq!(g.times_fired(1), 4);

        // Every second day.
        let mut e = many(global());
        (e.start_time, e.repeat, e.duration) = (0, 2 * DAY as u16, 2);
        let mut g = engine(vec![e]);
        assert!(tick_at(&mut g, &mut w, DAY + 10).is_empty());
        assert_eq!(tick_at(&mut g, &mut w, 2 * DAY + 10), vec![1]);

        // The stored duration is taken as hours: an editor window of 60 (one hour) lasts 60
        // hours.
        let mut e = many(global());
        (e.start_time, e.repeat, e.duration) = (0, 0, 60);
        let mut g = engine(vec![e]);
        assert_eq!(tick_at(&mut g, &mut w, 59 * 60), vec![1]);
        assert_eq!(tick_at(&mut g, &mut w, 60 * 60), vec![1]);
        assert!(tick_at(&mut g, &mut w, 60 * 60 + 1).is_empty());
    }

    /// Duration 0 (0x4a7b80): open with no end until it first fires, then until start + 60;
    /// its guard needs *last fired* below now − 60, and firing sets it to now + 1, so it fires
    /// again at the earliest 61 minutes later.
    #[test]
    fn a_duration_0_event_waits_61_minutes() {
        let mut once = many(global());
        (once.start_time, once.repeat, once.duration) = (1000, 0, 1);
        let mut open = many(global());
        (open.start_time, open.repeat, open.duration) = (1000, 0, 0);
        let mut g = engine(vec![once, open]);
        let mut w = MockWorld::new();
        assert_eq!(tick_at(&mut g, &mut w, 1000), vec![1, 2]);
        assert_eq!(tick_at(&mut g, &mut w, 1030), vec![1], "duration 0: the guard holds");
        assert!(tick_at(&mut g, &mut w, 1061).is_empty(), "both windows closed");
        let mut late = many(global());
        (late.start_time, late.repeat, late.duration) = (100, 0, 0);
        let mut g = engine(vec![late]);
        assert_eq!(tick_at(&mut g, &mut w, 5 * DAY), vec![1], "never fired: open with no end");
        assert!(tick_at(&mut g, &mut w, 5 * DAY + 61).is_empty(), "fired: its hour after the start is long past");
        // Daily and duration 0: one hour a day, at most once in it.
        let mut daily = many(global());
        (daily.start_time, daily.repeat, daily.duration) = (0, DAY as u16, 0);
        let mut g = engine(vec![daily]);
        assert_eq!(tick_at(&mut g, &mut w, DAY), vec![1]);
        assert!(tick_at(&mut g, &mut w, DAY + 60).is_empty(), "60 minutes are not enough");
        assert!(tick_at(&mut g, &mut w, DAY + 61).is_empty(), "61: its hour is over");
        // At the start of the game (the clock reads under 61), the file's last fired of 0
        // keeps a duration-0 event shut.
        let mut first = many(global());
        (first.start_time, first.repeat, first.duration) = (0, 0, 0);
        let mut g = engine(vec![first]);
        assert!(tick_at(&mut g, &mut w, 60).is_empty());
        assert_eq!(tick_at(&mut g, &mut w, 61), vec![1]);
    }

    #[test]
    fn once_versus_many() {
        let mut g = engine(vec![global(), many(global())]);
        let mut w = MockWorld::new();
        for day in 0..3 {
            tick_at(&mut g, &mut w, day * DAY + 5);
            tick_at(&mut g, &mut w, day * DAY + 600);
        }
        assert_eq!((g.times_fired(1), g.times_fired(2)), (1, 6), "a many-event fires on every check");
    }

    type Setup = fn(&mut Event);
    type World = fn(&mut MockWorld);

    /// Each condition group: (name, make the event, a world that passes, a world that fails).
    fn condition_cases() -> Vec<(&'static str, Setup, World, World)> {
        vec![
            ("archetype", |e| e.archetype = 2, |w| w.archetype = 2, |w| w.archetype = 3),
            ("squads at least", |e| (e.conditions.stats_check, e.conditions.squad_count) = (1, 3), |w| w.squads = 3, |w| w.squads = 2),
            ("squads at most", |e| (e.conditions.stats_check, e.conditions.squad_count) = (1, -3), |w| w.squads = 3, |w| w.squads = 4),
            ("strength", |e| (e.conditions.stats_check, e.conditions.army_strength) = (1, 1500), |w| w.strength = 1600, |w| w.strength = 1400),
            ("level", |e| (e.conditions.stats_check, e.conditions.level) = (1, 5), |w| w.level = 5, |w| w.level = 4),
            ("gold at most", |e| (e.conditions.stats_check, e.conditions.gold) = (1, -209), |w| w.gold = 209, |w| {
                w.gold = 210
            }),
            ("mana", |e| (e.conditions.stats_check, e.conditions.holiness_mana) = (1, 40), |w| w.mana = 40, |w| {
                w.mana = 39
            }),
            ("hero at 1 HP", |e| e.results.hero_one_hp = 1, |w| w.hp = 1, |w| w.hp = 2),
            (
                "building of the player",
                |e| {
                    let c = &mut e.conditions;
                    (c.buildings_check, c.buildings, c.buildings_owner) = (1, [8, 0, 0], [1, 0, 0]);
                },
                |w| _ = w.building_owner.insert(8, 1),
                |w| _ = w.building_owner.insert(8, 5),
            ),
            (
                "building of red",
                |e| {
                    let c = &mut e.conditions;
                    (c.buildings_check, c.buildings, c.buildings_owner) = (1, [8, 0, 0], [5, 0, 0]);
                },
                |w| _ = w.building_owner.insert(8, 5),
                |w| _ = w.building_owner.insert(8, 1),
            ),
            (
                "building of green: the player's side, his own buildings too (faction 1)",
                |e| {
                    let c = &mut e.conditions;
                    (c.buildings_check, c.buildings, c.buildings_owner) = (1, [8, 0, 0], [2, 0, 0]);
                },
                |w| _ = w.building_owner.insert(8, 1),
                |w| _ = w.building_owner.insert(8, 4),
            ),
            (
                "three alike with the player: three units",
                |e| {
                    let c = &mut e.conditions;
                    (c.units_check, c.units, c.units_owner) = (1, [60, 60, 60], [1, 1, 1]);
                },
                |w| w.units.extend([unit(60, 0); 3]),
                |w| w.units.extend([unit(60, 0); 2]),
            ),
            (
                "none alike with the player",
                |e| {
                    let c = &mut e.conditions;
                    (c.units_check, c.units, c.units_owner) = (1, [60, 60, 60], [6, 6, 6]);
                },
                |_| {},
                |w| w.units.push(unit(60, 0)),
            ),
            (
                "a dead unit is not with the player",
                |e| {
                    let c = &mut e.conditions;
                    (c.units_check, c.units, c.units_owner) = (1, [60, 0, 0], [1, 0, 0]);
                },
                |w| w.units.push(unit(60, 0)),
                |w| w.units.push(UnitRecord { hp: 0, ..unit(60, 0) }),
            ),
            (
                "an unnamed slot takes a named character of its type (the unit's name is not read)",
                |e| {
                    let c = &mut e.conditions;
                    (c.units_check, c.units, c.units_owner) = (1, [74, 0, 0], [1, 0, 0]);
                },
                |w| w.units.push(unit(74, 3)),
                |w| w.units.push(unit(75, 3)),
            ),
            (
                "a named slot takes its character whatever his type",
                |e| {
                    let c = &mut e.conditions;
                    (c.units_check, c.units, c.units_named, c.units_owner) = (1, [74, 0, 0], [2, 0, 0], [1, 0, 0]);
                },
                |w| w.units.push(unit(9, 2)),
                |w| w.units.push(unit(74, 0)),
            ),
            (
                "a unit with a faction's army, dead or alive",
                |e| {
                    let c = &mut e.conditions;
                    (c.units_check, c.units, c.units_owner) = (1, [74, 0, 0], [4, 0, 0]);
                },
                |w| _ = w.armies.insert(3, vec![vec![UnitRecord { hp: 0, ..unit(74, 0) }]]),
                |w| _ = w.armies.insert(2, vec![vec![unit(74, 0)]]),
            ),
            (
                "building not the player's",
                |e| {
                    let c = &mut e.conditions;
                    (c.buildings_check, c.buildings, c.buildings_owner) = (1, [8, 0, 0], [6, 0, 0]);
                },
                |_| {},
                |w| _ = w.building_owner.insert(8, 1),
            ),
            (
                "artifacts with the player",
                |e| {
                    let c = &mut e.conditions;
                    (c.artifacts_check, c.artifacts, c.artifacts_owner) = (1, [43, 44, 0], [1, 1, 0]);
                },
                |w| (w.pack, w.worn) = (vec![43], vec![44]),
                |w| w.pack = vec![43],
            ),
            (
                "one copy for each slot",
                |e| {
                    let c = &mut e.conditions;
                    (c.artifacts_check, c.artifacts, c.artifacts_owner) = (1, [43, 43, 0], [1, 1, 0]);
                },
                |w| (w.pack, w.worn) = (vec![43], vec![43]),
                |w| w.pack = vec![43],
            ),
            (
                "an artifact a faction's army wears (not one it carries)",
                |e| {
                    let c = &mut e.conditions;
                    (c.artifacts_check, c.artifacts, c.artifacts_owner) = (1, [43, 0, 0], [3, 0, 0]);
                },
                |w| _ = w.ai_worn.insert(2, vec![43]),
                |w| _ = w.ai_worn.insert(1, vec![43]),
            ),
            (
                "player defeated armies",
                |e| (e.conditions.defeated_check, e.conditions.defeated_armies) = (1, [3, 4]),
                |w| w.defeated.extend([3, 4]),
                |w| _ = w.defeated.insert(3),
            ),
            (
                "army beaten by anyone",
                |e| (e.conditions.beaten_check, e.conditions.beaten_armies) = (1, [3, 0]),
                |w| _ = w.beaten.insert(3),
                |_| {},
            ),
            ("army active", |e| e.conditions.army_active = 4, |w| _ = w.active.insert(4), |_| {}),
            ("army inactive", |e| e.conditions.army_inactive = 4, |w| _ = w.inactive.insert(4), |_| {}),
            ("army at home", |e| e.conditions.army_at_home = 13, |w| _ = w.home.insert(13), |_| {}),
        ]
    }

    #[test]
    fn conditions_by_group() {
        for (name, setup, pass, fail) in condition_cases() {
            let mut e = global();
            setup(&mut e);
            for (world, expect) in [(pass, true), (fail, false)] {
                let mut w = MockWorld::new();
                world(&mut w);
                let mut g = engine(vec![e.clone()]);
                assert_eq!(!tick_at(&mut g, &mut w, 0).is_empty(), expect, "{name}");
            }
        }
    }

    /// Byte 145 is a condition, never a result: the hero's HP is not touched.
    #[test]
    fn the_one_hp_byte_changes_nothing() {
        let mut e = global();
        (e.results.hero_one_hp, e.results.gold) = (1, 5);
        let mut g = engine(vec![e]);
        let mut w = MockWorld::new();
        w.hp = 1;
        assert_eq!(tick_at(&mut g, &mut w, 0), vec![1]);
        assert_eq!((w.hp, w.log.clone()), (1, vec![Fx::Gold(5)]));
    }

    /// The numeric conditions need "current stats" (byte 18), and the Community gate skips
    /// all five when the patrol value is set and the squad value is positive (c2698c).
    #[test]
    fn numeric_conditions_are_gated() {
        let mut squads = global();
        squads.conditions.squad_count = 5;
        squads.conditions.army_strength = 9000;
        let mut gated = global();
        let c = &mut gated.conditions;
        (c.stats_check, c.squad_count, c.gold) = (1, 5, 1000);
        (gated.results.patrol_army, gated.results.patrol_delta) = (3, 1);
        let mut g = engine(vec![squads, gated]);
        let mut w = MockWorld::new();
        assert_eq!(tick_at(&mut g, &mut w, 0), vec![1, 2], "no squads, no gold: neither is tested");
    }

    /// The units a named-squad check takes go into a list of three (0x4a8237): from the
    /// fourth on, each overwrites the third, which frees the unit noted there. Codes 2–5
    /// note one unit in every army of the faction that has one.
    #[test]
    fn the_units_taken_are_noted_in_three_places() {
        let mut e = global();
        let c = &mut e.conditions;
        (c.units_check, c.units, c.units_owner) = (1, [60, 60, 0], [2, 2, 0]);
        let mut g = engine(vec![e]);
        let mut w = MockWorld::new();
        // Faction 1: three armies of one such unit each. Slot 1 notes all three; slot 2
        // finds none left.
        w.armies.insert(1, vec![vec![unit(60, 0)]; 3]);
        assert!(tick_at(&mut g, &mut w, 0).is_empty());
        // A fourth army: slot 1's fourth unit overwrites the third army's, which slot 2
        // then takes.
        w.armies.insert(1, vec![vec![unit(60, 0)]; 4]);
        assert_eq!(tick_at(&mut g, &mut w, 0), vec![1]);
    }

    /// Owner code 0 (the list's empty first entry): a building slot is left out; a unit or
    /// artifact slot fails.
    #[test]
    fn owner_code_0() {
        let mut b = global();
        let c = &mut b.conditions;
        (c.buildings_check, c.buildings, c.buildings_owner) = (1, [8, 9, 0], [0, 1, 0]);
        let mut u = global();
        let c = &mut u.conditions;
        (c.units_check, c.units, c.units_owner) = (1, [60, 0, 0], [0, 0, 0]);
        let mut a = global();
        let c = &mut a.conditions;
        (c.artifacts_check, c.artifacts, c.artifacts_owner) = (1, [43, 0, 0], [0, 0, 0]);
        let mut g = engine(vec![b, u, a]);
        let mut w = MockWorld::new();
        w.building_owner.insert(9, 1);
        w.units.push(unit(60, 0));
        w.pack.push(43);
        assert_eq!(tick_at(&mut g, &mut w, 0), vec![1]);
    }

    /// Event points fire every event they list, a quest or a rumour too; villages and
    /// shipyards fire every event on entering; other buildings list their quests and rumours
    /// in the main hall.
    #[test]
    fn where_quests_and_rumours_fire() {
        let rumour = with_message(ev(EventKind::Rumour));
        let quest = with_message(ev(EventKind::Quest));
        let mut g = EventEngine::from_parts(
            vec![rumour.clone(), rumour, quest.clone(), quest],
            vec![(Place::Building(2), vec![1]), (Place::Building(3), vec![2, 3]), (Place::Point(7), vec![4])],
            0,
            0,
        );
        g.set_every_kind(Place::Building(2));
        let mut w = MockWorld::new();
        w.place = Some(Place::Building(3));
        assert!(tick_at(&mut g, &mut w, 0).is_empty());
        assert_eq!(g.hall(&w), vec![2, 3]);
        w.place = Some(Place::Building(2));
        assert_eq!(tick_at(&mut g, &mut w, 10), vec![1]);
        assert!(g.hall(&w).is_empty(), "a village has no hall list");
        w.place = Some(Place::Point(7));
        assert_eq!(tick_at(&mut g, &mut w, 20), vec![4]);
    }

    #[test]
    fn a_cleared_check_byte_disables_its_ids() {
        let mut e = global();
        e.conditions.defeated_armies = [3, 0];
        e.conditions.not_happened = [1, 0];
        let mut g = engine(vec![e]);
        assert_eq!(tick_at(&mut g, &mut MockWorld::new(), 0), vec![1]);
    }

    #[test]
    fn event_history_conditions() {
        let mut first = global();
        first.start_time = 100;
        let mut yes = global();
        (yes.conditions.happened_yes_check, yes.conditions.happened_yes) = (1, [1, 0]);
        let mut not = global();
        (not.conditions.not_happened_check, not.conditions.not_happened) = (1, [1, 0]);
        // 1 fires at 100; 3 (1 not happened) only before that, 2 (1 happened) only after.
        let mut g = engine(vec![first, yes, not]);
        let mut w = MockWorld::new();
        assert_eq!(tick_at(&mut g, &mut w, 0), vec![3]);
        assert_eq!(tick_at(&mut g, &mut w, 100), vec![1, 2]);
    }

    #[test]
    fn flags_from_the_title_script() {
        let events = vec![
            titled(global(), "+Foo"),      // 1: sets Foo
            titled(global(), "=Foo"),      // 2: needs Foo
            titled(global(), "-Foo=Foo"),  // 3: needs Foo, clears it
            titled(global(), "=/Foo"),     // 4: needs Foo unset
            titled(global(), "+Bar=/Foo"), // 5: needs Foo unset, sets Bar
        ];
        let mut g = engine(events);
        let mut w = MockWorld::new();
        assert_eq!(tick_at(&mut g, &mut w, 0), vec![1, 2, 3, 4, 5]);
        assert!(!g.flag("Foo"));
        assert!(g.flag("Bar"));
        assert_eq!(g.flag_string(), "Bar\u{a0}");
    }

    #[test]
    fn flag_condition_blocks_until_set() {
        let mut g = engine(vec![titled(global(), "=Foo"), titled(global(), "+Foo")]);
        let mut w = MockWorld::new();
        // The first matching event fires, then the loop starts over: 2 enables 1.
        assert_eq!(tick_at(&mut g, &mut w, 0), vec![2, 1]);
    }

    /// The flag string as the original keeps it (events.md §5): tests are substring searches,
    /// `+X` appends X only when it does not occur, `-X` removes the first occurrence and the
    /// character after it, and only the `^` form keeps a digit.
    #[test]
    fn flags_are_one_string_searched_for_substrings() {
        let mut g = engine(Vec::new());
        g.flag_action("+AB1");
        g.flag_action("+NoKey");
        assert!(g.flag_test("AB") && g.flag_test("Key") && g.flag_test("B1"), "substrings");
        assert!(!g.flag_test("/Key"));
        g.flag_action("+Key");
        assert_eq!(g.flag_string(), "AB1\u{a0}NoKey\u{a0}", "Key occurs already: nothing added");
        g.flag_action("-Key");
        assert_eq!(g.flag_string(), "AB1\u{a0}No", "the first occurrence and the space after it");
        g.flag_action("+A");
        g.flag_action("-AB");
        assert_eq!(g.flag_string(), "\u{a0}No", "two characters: nothing; -AB takes AB and the 1");
        // Tests: the first ^ is dropped; an empty test or end_tutorial passes; / negates.
        assert!(g.flag_test("") && g.flag_test("^") && g.flag_test("/") && g.flag_test("x end_tutorial"));
        assert!(g.flag_test("N^o") && !g.flag_test("/No"));

        // Counters: +X^ appends X1 or raises the character after X; -X^ lowers it and
        // removes the entry at 0.
        let mut g = engine(Vec::new());
        g.flag_action("+Foo^");
        g.flag_action("+Foo^");
        assert_eq!(g.flag_string(), "Foo2\u{a0}");
        assert!(g.flag_test("Foo2") && !g.flag_test("Foo1"));
        g.flag_action("-Foo^");
        g.flag_action("-Foo^");
        assert_eq!(g.flag_string(), "");
        // -X^ without X changes nothing (the original's bug lowered the character at X's
        // length: "Aacd").
        g.set_flag_string("Abcd\u{a0}");
        g.flag_action("-Zz^");
        assert_eq!(g.flag_string(), "Abcd\u{a0}");
    }

    /// Saves before format 8 kept a list of flag names.
    #[test]
    fn old_saves_keep_their_flags() {
        let g = engine(Vec::new());
        let mut v = serde_json::to_value(&g).unwrap();
        v["flags"] = serde_json::json!(["Ключ1", "Bar"]);
        let old: EventEngine = serde_json::from_value(v).unwrap();
        assert_eq!(old.flag_string(), "Ключ1\u{a0}Bar\u{a0}");
        let again: EventEngine = serde_json::from_str(&serde_json::to_string(&old).unwrap()).unwrap();
        assert_eq!(again.flag_string(), old.flag_string());
    }

    #[test]
    fn engine_flags_sea_and_shipyard() {
        let mut g = engine(vec![titled(global(), "=Sea")]);
        g.set_engine_flag("Sea", true);
        g.set_engine_flag("Sea", true);
        assert_eq!(g.flag_string(), "Sea\u{a0}");
        let mut w = MockWorld::new();
        assert_eq!(tick_at(&mut g, &mut w, 0), vec![1]);
        g.set_engine_flag("Sea", false);
        assert_eq!(g.flag_string(), "");
    }

    #[test]
    fn chained_subordinate_events() {
        let mut parent = global();
        parent.start_time = 100;
        parent.results.chained_event = 2;
        let mut sub = with_message(ev(EventKind::Local));
        (sub.subordinate, sub.once, sub.repeat, sub.duration) = (1, 0, 0, 0);
        (sub.conditions.happened_yes_check, sub.conditions.happened_yes) = (1, [1, 0]);
        sub.results.gold = 50;
        sub.results.chained_event = 3;
        // 3 is chained although its own conditions fail: the original runs a chained event
        // without checking it (4ab1ec hands it to the dialog or applies it at once).
        let mut unchecked = global();
        unchecked.subordinate = 1;
        unchecked.conditions.meet_army = 9;
        unchecked.results.gold = 7;
        let mut g = engine(vec![parent, sub, unchecked]);
        let mut w = MockWorld::new();
        assert!(tick_at(&mut g, &mut w, 0).is_empty(), "a subordinate event never fires on its own");
        w.now = 100;
        let out = g.tick(&mut w);
        let out = read(&mut g, &mut w, out);
        assert_eq!(fired(&out), vec![1, 2, 3]);
        assert!(out.contains(&EventOutcome::Fired { event: 2, message: true }));
        assert_eq!(w.log, vec![Fx::Gold(50), Fx::Gold(7)]);
    }

    /// FINDINGS §22 (РК1's church): a shown window stops the scan (0x4ac3b4); the events after
    /// it, a textless one too, run only when it is closed (OK → 0x4ab1ec → a new scan), each
    /// window in turn. The chain of a shown event waits for its window as well.
    #[test]
    fn a_shown_window_holds_the_scan_until_it_is_closed() {
        let first = with_message(global());
        let mut second = with_message(global());
        (second.conditions.happened_yes_check, second.conditions.happened_yes) = (1, [1, 0]);
        let mut silent = global();
        (silent.conditions.happened_yes_check, silent.conditions.happened_yes) = (1, [2, 0]);
        silent.results.activate_armies[0] = 2;
        let mut g = engine(vec![first, second, silent]);
        let mut w = MockWorld::new();
        assert_eq!(fired(&g.tick(&mut w)), vec![1]);
        assert!(g.holds_window());
        assert!(fired(&g.tick(&mut w)).is_empty(), "time does not run the scan on");
        assert_eq!(fired(&g.window_closed(&mut w)), vec![2]);
        assert!(w.log.is_empty(), "the army waits for the second window");
        assert_eq!(fired(&g.window_closed(&mut w)), vec![3]);
        assert_eq!(w.log, vec![Fx::Activate(2)]);
        assert!(!g.holds_window());
        assert!(g.window_closed(&mut w).is_empty());

        let mut shown = with_message(global());
        shown.results.chained_event = 2;
        let mut chained = global();
        (chained.subordinate, chained.results.gold) = (1, 5);
        let mut g = engine(vec![shown, chained]);
        let mut w = MockWorld::new();
        assert_eq!(fired(&g.tick(&mut w)), vec![1]);
        assert!(w.log.is_empty());
        assert_eq!(fired(&g.window_closed(&mut w)), vec![2]);
        assert_eq!(w.log, vec![Fx::Gold(5)]);
    }

    /// РК1's ending on the "No" path: a local event chains the victory event, whose own
    /// conditions (a Yes to the herald) do not hold. It still fires, and so does a used-up
    /// once-event chained again.
    #[test]
    fn a_chained_event_fires_whatever_its_conditions_and_once_flag() {
        let ask = asking(global());
        let mut arrive = global();
        (arrive.conditions.happened_no_check, arrive.conditions.happened_no) = (1, [1, 0]);
        arrive.results.chained_event = 3;
        let mut leave = global();
        (leave.conditions.happened_yes_check, leave.conditions.happened_yes) = (1, [1, 0]);
        let mut g = EventEngine::from_parts(vec![ask, arrive, leave], Vec::new(), 3, 0);
        let mut w = MockWorld::new();
        w.now = 5;
        let out = g.tick(&mut w);
        assert_eq!(out, vec![EventOutcome::Question(1)]);
        let out = g.answer(&mut w, false);
        assert_eq!(fired(&out), vec![2, 3]);
        assert_eq!(g.ended(), Some(&EventOutcome::Victory(3)));

        let mut once = global();
        once.subordinate = 1;
        once.results.gold = 3;
        let mut again = many(global());
        again.results.chained_event = 1;
        let mut g = engine(vec![once, again]);
        let mut w = MockWorld::new();
        assert_eq!(tick_at(&mut g, &mut w, 10), vec![2, 1]);
        assert_eq!(tick_at(&mut g, &mut w, 20), vec![2, 1], "chained again although fired once");
    }

    /// The follow-ups (0x4ab1ec tail): a delay drops the chain unless lanterns, a shown army
    /// or a spell start the chain timer; a battle does not stop it; a victory or defeat
    /// event stops before any follow-up.
    #[test]
    fn follow_ups_and_chains() {
        let mut sub = global();
        (sub.subordinate, sub.results.gold) = (1, 1);
        let mut delayed = global();
        (delayed.results.delay_hours, delayed.results.chained_event) = (2, 1);
        let mut g = engine(vec![sub.clone(), delayed.clone()]);
        let mut w = MockWorld::new();
        assert_eq!(tick_at(&mut g, &mut w, 0), vec![2], "the chain behind the delay is dropped");
        assert_eq!(w.log, vec![Fx::Delay(120)]);

        delayed.results.cast_spell = 4;
        let mut fight = global();
        (fight.results.start_battle_with, fight.results.chained_event) = (6, 1);
        let mut g = engine(vec![sub.clone(), delayed, fight]);
        let mut w = MockWorld::new();
        assert_eq!(tick_at(&mut g, &mut w, 0), vec![2, 1, 3, 1]);
        assert_eq!(w.log, vec![Fx::Spell(4), Fx::Delay(120), Fx::Gold(1), Fx::Battle(6), Fx::Gold(1)]);

        let mut win = global();
        let r = &mut win.results;
        (r.cast_spell, r.start_battle_with, r.delay_hours, r.chained_event, r.gold) = (4, 6, 1, 1, 9);
        let mut g = EventEngine::from_parts(vec![sub, win], Vec::new(), 2, 0);
        let mut w = MockWorld::new();
        assert_eq!(tick_at(&mut g, &mut w, 0), vec![2]);
        assert_eq!(w.log, vec![Fx::Gold(9)], "no spell, battle, delay or chain after the victory");
    }

    /// The original's "meet army" is the army met right now (0x68dc7c): set when the hero
    /// meets it, cleared when the run that followed ends (0x4ac39e) or by a "no meeting"
    /// result (0x4ab286). A meeting event does not fire again later without a new meeting.
    #[test]
    fn a_meeting_holds_for_the_run_it_starts_only() {
        let mut talk = with_message(many(global()));
        talk.conditions.meet_army = 1;
        let mut greet = many(global());
        (greet.conditions.meet_army, greet.results.gold) = (1, 5);
        let mut g = engine(vec![talk, greet]);
        let mut w = MockWorld::new();
        assert!(tick_at(&mut g, &mut w, 0).is_empty());
        w.now = 10;
        assert_eq!(fired(&meet_read(&mut g, &mut w, 1)), vec![1, 2]);
        assert!(tick_at(&mut g, &mut w, 20).is_empty(), "no meeting now");
        w.now = 30;
        assert!(fired(&meet_read(&mut g, &mut w, 2)).is_empty(), "another army");
        w.now = 40;
        assert_eq!(fired(&meet_read(&mut g, &mut w, 1)), vec![1, 2], "met again");

        // "No meeting" ends the meeting: later events of the same run do not see it.
        let mut first = many(global());
        (first.conditions.meet_army, first.results.no_meeting) = (1, 1);
        let mut second = many(global());
        second.conditions.meet_army = 1;
        let mut g = engine(vec![first, second]);
        let mut w = MockWorld::new();
        w.now = 10;
        assert_eq!(fired(&meet_read(&mut g, &mut w, 1)), vec![1]);
    }

    /// The "meeting event waiting" mark (0x4a801a): set when every condition before the
    /// meet-army test holds (an active-army test is one of them; the hero's HP is not).
    #[test]
    fn the_meeting_mark_reads_the_conditions_before_the_meet_test() {
        let mut talk = global();
        (talk.conditions.meet_army, talk.conditions.army_active) = (4, 9);
        talk.results.hero_one_hp = 1;
        let g = engine(vec![talk]);
        let mut w = MockWorld::new();
        assert!(!g.meeting_waiting(&w, 4), "army 9 is not active");
        w.active.insert(9);
        assert!(g.meeting_waiting(&w, 4), "the HP test comes after the meeting");
        assert!(!g.meeting_waiting(&w, 5));
    }

    /// A question asked at a meeting keeps the meeting for the rest of the run.
    #[test]
    fn a_meeting_lasts_through_its_question() {
        let mut ask = asking(global());
        ask.conditions.meet_army = 4;
        let mut then = global();
        (then.conditions.meet_army, then.conditions.happened_yes_check, then.conditions.happened_yes) = (4, 1, [1, 0]);
        let mut g = engine(vec![ask, then]);
        let mut w = MockWorld::new();
        w.now = 10;
        assert_eq!(g.meet(&mut w, 4), vec![EventOutcome::Question(1)]);
        assert_eq!(fired(&g.answer(&mut w, true)), vec![1, 2]);
    }

    /// Deactivating the army met ends the meeting (0x496900): the first tutorial map's ghost,
    /// sent away by a Yes, is not met again in the same run.
    #[test]
    fn a_meeting_ends_when_its_army_is_deactivated() {
        let mut ask = asking(global());
        (ask.conditions.meet_army, ask.results.deactivate_army) = (4, 4);
        let mut then = with_message(global());
        (then.conditions.meet_army, then.conditions.happened_yes_check, then.conditions.happened_yes) = (4, 1, [1, 0]);
        let mut g = engine(vec![ask, then]);
        let mut w = MockWorld::new();
        w.now = 10;
        assert_eq!(g.meet(&mut w, 4), vec![EventOutcome::Question(1)]);
        assert_eq!(fired(&g.answer(&mut w, true)), vec![1]);
        assert_eq!(g.meeting(), None);
        // Met again later (event 8 brought it back): now event 2 fires.
        w.now = 100;
        assert_eq!(fired(&meet_read(&mut g, &mut w, 4)), vec![2]);
    }

    #[test]
    fn relative_event_start_is_set_by_another() {
        let mut trigger = global();
        trigger.start_time = 600;
        (trigger.results.relative_event, trigger.results.relative_delay_hours) = (2, 5);
        let mut later = with_message(global());
        later.start_time = 1_036_800_000; // "never" until moved
        let mut g = engine(vec![trigger, later]);
        let mut w = MockWorld::new();
        assert!(tick_at(&mut g, &mut w, 0).is_empty());
        assert_eq!(tick_at(&mut g, &mut w, 610), vec![1]);
        assert_eq!(g.start_time(2), Some(610 + 300));
        assert!(tick_at(&mut g, &mut w, 900).is_empty());
        assert_eq!(tick_at(&mut g, &mut w, 910), vec![2]);
    }

    #[test]
    fn questions_yes_and_no_paths() {
        let mut ask = with_message(asking(global()));
        ask.results.gold = -150;
        ask.results.artifacts_add = [14, 0, 0, 0];
        let mut on_yes = global();
        (on_yes.conditions.happened_yes_check, on_yes.conditions.happened_yes) = (1, [1, 0]);
        let mut on_no = global();
        (on_no.conditions.happened_no_check, on_no.conditions.happened_no) = (1, [1, 0]);
        let mut not = global();
        (not.conditions.not_happened_check, not.conditions.not_happened) = (1, [1, 0]);
        not.start_time = 20;

        // No: nothing is applied, but it counts as happened: the No branch runs and a
        // once-question is used up.
        let events = vec![ask, on_yes, on_no, not];
        let mut g = engine(events.clone());
        let mut w = MockWorld { gold: 300, ..MockWorld::new() };
        w.now = 10;
        assert_eq!(g.tick(&mut w), vec![EventOutcome::Question(1)]);
        assert_eq!(g.pending_question(), Some(1));
        assert!(g.tick(&mut w).is_empty(), "nothing runs while a question waits");
        let out = g.answer(&mut w, false);
        assert_eq!(out[0], EventOutcome::Declined(1));
        assert_eq!(fired(&out), vec![3]);
        assert_eq!((g.happened(1), g.times_fired(1)), (Some(Answer::No), 1));
        assert!(w.log.is_empty());
        assert!(tick_at(&mut g, &mut w, DAY + 10).is_empty(), "used up; 'not happened' does not hold");

        // Yes: the window's results (the artifact), then the rest at OK (the gold); the Yes
        // branch runs; the event is done.
        let mut g = engine(events);
        w.now = 10;
        assert_eq!(g.tick(&mut w), vec![EventOutcome::Question(1)]);
        let out = g.answer(&mut w, true);
        let out = read(&mut g, &mut w, out);
        assert_eq!(fired(&out), vec![1, 2]);
        assert_eq!(w.log, vec![Fx::GiveItem(14), Fx::Gold(-150)]);
        assert_eq!(g.happened(1), Some(Answer::Yes));
        assert!(tick_at(&mut g, &mut w, 3 * DAY).is_empty());
    }

    /// A No uses the guard of one scan only: the question comes back on the next scan, the
    /// No still standing until the event opens without its question.
    #[test]
    fn a_many_question_answered_no_comes_back_on_the_next_scan() {
        let ask = many(asking(global()));
        let mut g = engine(vec![ask]);
        let mut w = MockWorld::new();
        assert_eq!(g.tick(&mut w), vec![EventOutcome::Question(1)]);
        g.answer(&mut w, false);
        assert_eq!(g.tick(&mut w), vec![EventOutcome::Question(1)], "the idle reset opened it");
        assert_eq!(g.happened(1), Some(Answer::No), "asking does not clear the No");
        g.answer(&mut w, true);
        assert_eq!(g.happened(1), Some(Answer::Yes));
    }

    /// Yes (0x4c2100): once := not "repeat after yes" (149). Without 149 an answered event
    /// is done, whatever its once box; with it the event repeats and asks again, with a
    /// message or without (the original's bug: without one its write missed the event,
    /// which then fired without its question).
    #[test]
    fn a_yes_rewrites_once_and_ask() {
        let mut w = MockWorld::new();
        for (message, repeat, again) in [(true, 0, false), (false, 0, false), (true, 1, true), (false, 1, true)] {
            let mut e = many(asking(global()));
            e.results.repeat_after_yes = repeat;
            if message {
                e = with_message(e);
            }
            let mut g = engine(vec![e]);
            w.now = 0;
            assert_eq!(g.tick(&mut w), vec![EventOutcome::Question(1)]);
            let out = g.answer(&mut w, true);
            read(&mut g, &mut w, out);
            w.now = DAY;
            let out = g.tick(&mut w);
            if again {
                assert_eq!(out, vec![EventOutcome::Question(1)], "asks again ({message})");
            } else {
                assert!(out.is_empty(), "done after a Yes ({message}, {repeat})");
            }
        }
    }

    /// Yes writes once := byte 149 xor 1 (0x4c2189), a bit flip: a 149 of 2 gives once 3,
    /// which still closes the event.
    #[test]
    fn a_yes_flips_the_low_bit_of_149_into_once() {
        let mut w = MockWorld::new();
        for (repeat, once) in [(0, 1), (1, 0), (2, 3), (3, 2)] {
            let mut e = with_message(many(asking(global())));
            e.results.repeat_after_yes = repeat;
            let mut g = engine(vec![e]);
            g.tick(&mut w);
            g.answer(&mut w, true);
            assert_eq!(g.event_field(1, 141), Some(once), "149 = {repeat}");
        }
    }

    /// An asking event with an empty message: Yes finishes it at once, without its
    /// artifacts, units and spells (10 shipped events ask so).
    #[test]
    fn a_yes_without_a_message_skips_the_window_results() {
        let mut e = asking(global());
        let r = &mut e.results;
        (r.gold, r.artifacts_add, r.units_add, r.spells_learned) = (5, [3, 0, 0, 0], [60, 0, 0, 0], [2, 0, 0, 0]);
        let mut g = engine(vec![e]);
        let mut w = MockWorld::new();
        g.tick(&mut w);
        let out = g.answer(&mut w, true);
        assert_eq!(fired(&out), vec![1]);
        assert_eq!(w.log, vec![Fx::Gold(5)]);
    }

    /// An event with neither a question nor a message never asks (0x4a7a40).
    #[test]
    fn an_event_without_texts_never_asks() {
        let mut e = global();
        (e.conditions.confirm_question, e.results.gold) = (1, 5);
        let mut g = engine(vec![e]);
        let mut w = MockWorld::new();
        assert_eq!(tick_at(&mut g, &mut w, 0), vec![1]);
    }

    /// A quest is added to the journal every time it finishes; completing it removes its
    /// last entry; a completed quest can be received again.
    #[test]
    fn quest_journal_add_and_complete() {
        let quest = with_message(many(ev(EventKind::Quest)));
        let mut done = many(global());
        (done.conditions.defeated_check, done.conditions.defeated_armies) = (1, [4, 0]);
        done.results.completes_quest = 1;
        done.results.gold = 25;
        let mut g = EventEngine::from_parts(vec![quest, done], vec![(Place::Building(6), vec![1])], 0, 0);
        let mut w = MockWorld::new();
        w.place = Some(Place::Building(6));
        assert!(g.tick(&mut w).is_empty(), "a quest in a building waits in the main hall");
        assert_eq!(g.hall(&w), vec![1]);
        let out = g.take(&mut w, 1);
        assert_eq!(out, vec![EventOutcome::Fired { event: 1, message: true }, EventOutcome::QuestAdded(1)]);
        read(&mut g, &mut w, Vec::new());
        w.now = 10;
        let out = g.take(&mut w, 1);
        read(&mut g, &mut w, out);
        assert_eq!(g.journal(), &[1, 1], "no duplicate check");
        w.place = None;
        w.defeated.insert(4);
        let out = g.tick(&mut w);
        assert!(out.contains(&EventOutcome::QuestCompleted(1)));
        assert_eq!(g.journal(), &[1], "the last entry goes");
        assert_eq!(g.completed_quests(), &[1]);
        assert_eq!(w.gold, 25);
    }

    #[test]
    fn local_events_fire_at_points_on_every_check_and_in_buildings_on_entering() {
        let at = Place::Point(3);
        let inn = Place::Building(5);
        let mut g = EventEngine::from_parts(
            vec![many(ev(EventKind::Local)), global(), many(ev(EventKind::Local))],
            vec![(at, vec![1]), (inn, vec![3])],
            0,
            0,
        );
        let mut w = MockWorld::new();
        assert_eq!(tick_at(&mut g, &mut w, 0), vec![2], "the global fires anywhere, the local does not");
        w.place = Some(Place::Point(4));
        assert!(tick_at(&mut g, &mut w, 10).is_empty());
        w.place = Some(at);
        assert_eq!(tick_at(&mut g, &mut w, 20), vec![1]);
        assert_eq!(tick_at(&mut g, &mut w, 30), vec![1], "standing on the point: every check");
        w.place = Some(inn);
        assert_eq!(tick_at(&mut g, &mut w, 40), vec![3], "on entering");
        assert!(tick_at(&mut g, &mut w, 50).is_empty(), "not again while he is inside");
        w.place = None;
        tick_at(&mut g, &mut w, 60);
        w.place = Some(inn);
        assert_eq!(tick_at(&mut g, &mut w, 70), vec![3], "a new visit");
        g.visit(inn);
        assert_eq!(tick_at(&mut g, &mut w, 80), vec![3], "re-entered without leaving the cell");
    }

    /// The hall lists what passes the full check, the window and once flag included; taking
    /// an entry opens it without a new check.
    #[test]
    fn hall_entries_fire_only_when_taken() {
        let mut rumour = with_message(ev(EventKind::Rumour));
        rumour.results.gold = 75;
        let mut pricey = ev(EventKind::Rumour);
        (pricey.conditions.stats_check, pricey.conditions.gold) = (1, 1000);
        let mut late = ev(EventKind::Rumour);
        late.start_time = 5000;
        late.subordinate = 0;
        let inn = Place::Building(1);
        let mut g = EventEngine::from_parts(vec![rumour, pricey, late], vec![(inn, vec![1, 2, 3])], 0, 0);
        let mut w = MockWorld { place: Some(inn), ..MockWorld::new() };
        assert!(g.tick(&mut w).is_empty());
        assert_eq!(g.hall(&w), vec![1]);
        assert!(g.take(&mut w, 2).is_empty(), "not on offer");
        let out = g.take(&mut w, 1);
        assert_eq!(fired(&out), vec![1]);
        assert_eq!(w.gold, 75);
        assert!(g.hall(&w).is_empty(), "a once-rumour is heard once");
        w.place = None;
        assert!(g.hall(&w).is_empty());
    }

    #[test]
    fn a_flag_ping_pong_fires_each_once_a_scan_and_the_guard_stops_long_runs() {
        let mut g = engine(vec![many(titled(global(), "+AA=/AA")), many(titled(global(), "-AA=AA"))]);
        let mut w = MockWorld::new();
        assert_eq!(fired(&g.tick(&mut w)), vec![1, 2]);
        assert_eq!(tick_at(&mut g, &mut w, 1), vec![1, 2]);
        let mut g = engine(vec![many(global()); LOOP_GUARD + 10]);
        let out = g.tick(&mut w);
        assert_eq!(fired(&out).len(), LOOP_GUARD);
        assert_eq!(out.last(), Some(&EventOutcome::LoopGuard));
    }

    #[test]
    fn victory_and_defeat_events_end_the_scenario() {
        let mut win = global();
        (win.conditions.defeated_check, win.conditions.defeated_armies) = (1, [2, 0]);
        let mut lose = global();
        let c = &mut lose.conditions;
        (c.units_check, c.units, c.units_named, c.units_owner) = (1, [74, 0, 0], [1, 0, 0], [6, 0, 0]);
        let mut g = EventEngine::from_parts(vec![win.clone(), lose.clone(), global()], Vec::new(), 1, 2);
        let mut w = MockWorld::new();
        w.units.push(unit(74, 1));
        w.defeated.insert(2);
        let out = g.tick(&mut w);
        assert_eq!(out.last(), Some(&EventOutcome::Victory(1)));
        assert_eq!(g.ended(), Some(&EventOutcome::Victory(1)));
        assert!(tick_at(&mut g, &mut w, DAY).is_empty(), "nothing runs after the end");

        let mut g = EventEngine::from_parts(vec![win, lose], Vec::new(), 1, 2);
        let mut w = MockWorld::new();
        assert_eq!(g.tick(&mut w).last(), Some(&EventOutcome::Defeat(2)));
    }

    /// Every result through the world, in the original's order: the window's (artifacts
    /// gained, lost, units added, removed, spells learned), then the finish's (XP, gold,
    /// mana, armies, patrol, shown army, lanterns, the army moved, the class), then the
    /// follow-ups (battle, spell, delay).
    #[test]
    fn results_go_through_the_world() {
        let mut e = global();
        let r = &mut e.results;
        (r.experience, r.gold, r.mana) = (100, -20, 7);
        (r.patrol_army, r.patrol_delta) = (3, -2);
        r.cast_spell = 13;
        r.spells_learned = [4, 0, 0, 0];
        (r.units_add, r.units_add_named, r.units_from_army) = ([74, 0, 0, 0], [1, 0, 0, 0], 7);
        (r.units_remove, r.units_remove_named, r.removed_units_to_army) = ([0xFE, 0xFF, 42, 0], [0, 0, 1, 0], 19);
        (r.move_to_hero, r.units_from_army) = (1, 7);
        (r.artifacts_add, r.artifacts_remove) = ([43, 0, 0, 0], [111, 0, 0, 0]);
        (r.activate_armies, r.deactivate_army, r.show_army) = ([11, 12], 14, 18);
        r.light_lanterns = [2, 13, 0, 0];
        r.new_hero_class = 11;
        r.delay_hours = 2;
        r.start_battle_with = 8;
        let mut g = engine(vec![e]);
        let mut w = MockWorld { gold: 100, ..MockWorld::new() };
        w.pack.push(111);
        // The hero, an event unit, a plain unit, the named character of slot 3.
        w.units.extend([UnitRecord { from_event: true, ..unit(30, 0) }, unit(31, 0), unit(5, 1)]);
        tick_at(&mut g, &mut w, 0);
        let to = Some(19);
        assert_eq!(
            w.log,
            vec![
                Fx::GiveItem(43),
                Fx::TakeItem(111),
                Fx::AddUnit(74, 1, Some(7)),
                // 0xFE: the last unnamed event unit (the one just added is named); 0xFF: the
                // last unnamed unit; then the last unit carrying the slot's name.
                Fx::RemoveUnit(1, to),
                Fx::RemoveUnit(1, to),
                Fx::RemoveUnit(2, to),
                Fx::Learn(4),
                Fx::Xp(100),
                Fx::Gold(-20),
                Fx::Mana(7),
                Fx::Activate(11),
                Fx::Activate(12),
                Fx::Deactivate(14),
                Fx::Patrol(3, -2),
                Fx::Show(18),
                Fx::Lantern(2),
                Fx::Lantern(13),
                Fx::MoveToHero(19),
                Fx::Class(11),
                Fx::Battle(8),
                Fx::Spell(13),
                Fx::Delay(120),
            ]
        );
    }

    /// Artifacts lost: only when one of them is held; units removed: only when one slot
    /// finds a unit, searched from the last unit back.
    #[test]
    fn losses_need_something_to_lose() {
        let mut e = many(global());
        (e.results.artifacts_remove, e.results.units_remove) = ([5, 6, 0, 0], [40, 0, 0, 0]);
        let mut g = engine(vec![e]);
        let mut w = MockWorld::new();
        tick_at(&mut g, &mut w, 0);
        assert!(w.log.is_empty());
        w.pack.push(6);
        w.units.extend([unit(40, 0), unit(40, 2), unit(40, 0), unit(41, 0)]);
        tick_at(&mut g, &mut w, 10);
        assert_eq!(w.log, vec![Fx::TakeItem(5), Fx::TakeItem(6), Fx::RemoveUnit(3, None)]);
        // The hero is never removed by a type match *(guess)*.
        let mut w = MockWorld::new();
        let mut e = global();
        e.results.units_remove = [1, 0, 0, 0];
        tick_at(&mut engine(vec![e]), &mut w, 0);
        assert!(w.log.is_empty());
    }

    #[test]
    fn the_army_moved_to_the_hero_is_byte_136() {
        let mut e = many(global());
        (e.results.move_to_hero, e.results.units_from_army) = (1, 7);
        let mut g = engine(vec![e]);
        let mut w = MockWorld::new();
        tick_at(&mut g, &mut w, 0);
        assert!(w.log.is_empty(), "byte 142 does not count");
    }

    #[test]
    fn meeting_repeats_at_every_new_meeting() {
        let mut talk = many(global());
        talk.conditions.meet_army = 1;
        talk.results.no_meeting = 1;
        let mut g = engine(vec![talk]);
        let mut w = MockWorld::new();
        for n in 1..=3 {
            w.now = n * 10;
            g.meet(&mut w, 1);
            assert_eq!(g.times_fired(1), n as u32);
        }
    }

    /// The end of the tutorial: a test holding `end_tutorial` passes, and finishing the
    /// event marks the tutorial done.
    #[test]
    fn end_tutorial_marks_the_tutorial_done() {
        let mut g = engine(vec![titled(global(), "=end_tutorial")]);
        let mut w = MockWorld::new();
        assert!(!g.tutorial_done());
        assert_eq!(tick_at(&mut g, &mut w, 0), vec![1]);
        assert!(g.tutorial_done());
    }

    /// Texts as the game loads them: double spaces collapsed; the shown title cut at `%` and
    /// at `#`.
    #[test]
    fn texts_as_loaded_and_shown() {
        let e = Event { title: "A  title   # note%+Big  Flag".into(), message: "x   y".into(), ..Event::default() };
        let e = e.for_play();
        assert_eq!((e.title.as_str(), e.message.as_str()), ("A title # note%+Big Flag", "x y"));
        assert_eq!(e.flags.as_ref().map(|f| f.action()), Some("+Big Flag"));
        assert_eq!(e.display_title(), "A title");
    }

    #[test]
    fn community_extensions_are_listed_and_run() {
        let mut vanilla = global();
        (vanilla.results.no_meeting, vanilla.conditions.meet_army) = (1, 1);
        let mut edit = global();
        (edit.results.no_meeting, edit.results.patrol_delta, edit.results.gold) = (1, 2, 85);
        // "No meeting" with a spell is a plain event: the spell is cast.
        let mut spell = global();
        (spell.results.no_meeting, spell.results.cast_spell) = (1, 13);
        // Opcode mode needs "no meeting" = 1.
        let mut two = global();
        (two.results.no_meeting, two.results.patrol_delta, two.results.patrol_army, two.results.gold) = (2, 4, 3, 6);
        let mut g = engine(vec![vanilla, edit, spell, two]);
        assert_eq!(g.extensions(), &[(2, Extension::Opcode(2))]);
        let mut w = MockWorld::new();
        assert_eq!(fired(&meet_read(&mut g, &mut w, 1)), vec![1, 2, 3, 4]);
        assert_eq!(w.log, vec![Fx::Spell(13), Fx::Gold(6), Fx::Patrol(3, 4)], "no gold for the opcode");
    }

    #[test]
    fn an_event_with_no_meeting_1_teaches_no_spell() {
        // The Community hook 0xc28604 skips the learning whenever the "no meeting" byte is 1,
        // whether the event is an opcode or not; another value teaches.
        for (no_meeting, log) in [(1, vec![]), (2, vec![Fx::Learn(4)]), (0, vec![Fx::Learn(4)])] {
            let mut e = global();
            (e.results.no_meeting, e.results.spells_learned) = (no_meeting, [4, 0, 0, 0]);
            let mut g = engine(vec![e]);
            let mut w = MockWorld::new();
            tick_at(&mut g, &mut w, 0);
            assert_eq!(w.log, log, "no meeting {no_meeting}");
        }
    }

    /// A Community opcode event: "no meeting", patrol value `code`, resources (XP, gold, mana).
    fn op(code: i8, x: i16, g: i16, m: i16) -> Event {
        let mut e = global();
        (e.results.no_meeting, e.results.patrol_delta) = (1, code);
        (e.results.experience, e.results.gold, e.results.mana) = (x, g, m);
        e
    }

    /// An event only a chain fires, giving 5 gold.
    fn target_event() -> Event {
        let mut e = global();
        e.subordinate = 1;
        e.results.gold = 5;
        e
    }

    /// Opcodes 1 and 2 poke one byte (c2669e): of event (this + A), at offset B, past the
    /// record into the next ones; 1 and negative opcodes add (wrapping), 2 sets. The second
    /// poke runs for every opcode outside 6–22.
    #[test]
    fn opcodes_1_and_2_poke_bytes_of_other_events() {
        let mut second = op(1, 0, 0, 0);
        // Second setting: action 1 (add), shift −3, byte 83 (XP), value 100.
        (second.conditions.squad_count, second.conditions.gold, second.conditions.level, second.conditions.holiness_mana) = (1, -3, 83, 100);
        let mut third = op(30, 0, 0, 0);
        // Opcode 30 (outside 6–22): only the second setting, set (2) event 1's byte 90 (the
        // mana's high byte) to 1.
        (third.conditions.squad_count, third.conditions.gold, third.conditions.level, third.conditions.holiness_mana) = (2, -4, 90, 1);
        let events = vec![target_event(), op(1, -1, 85, 2), op(2, -2, 89, 40), second, third, op(-1, -5, 171 + 85, 250)];
        let mut g = engine(events.clone());
        let mut w = MockWorld::new();
        assert_eq!(tick_at(&mut g, &mut w, 0), vec![2, 3, 4, 5, 6]);
        assert_eq!([g.event_field(1, 85), g.event_field(1, 89), g.event_field(1, 83)], [Some(7), Some(256 + 40), Some(100)]);
        assert_eq!(g.event_field(2, 85), Some((85 + 250) % 256), "the next record's gold byte wraps; nothing carries over");
        assert_eq!(g.event_field(1, 86), None, "inside a field");
        assert!(w.log.is_empty() && w.gold == 0, "the resources are arguments: {:?}", w.log);
        // A save keeps the edits.
        let saved: EventEngine = serde_json::from_str(&serde_json::to_string(&g).unwrap()).unwrap();
        let mut loaded = saved;
        loaded.restore_statics(engine(events)).unwrap();
        assert_eq!(loaded.event_field(1, 85), Some(7));
        // The edited event gives 7 gold when chained.
        let mut chain = global();
        chain.results.chained_event = 1;
        let mut g = engine(vec![target_event(), op(1, -1, 85, 2), chain]);
        tick_at(&mut g, &mut w, 0);
        assert_eq!(w.log, vec![Fx::Gold(7)]);
    }

    /// Bytes 156–162 of a record are the engine's state (last fired, times fired, the
    /// answer): opcodes poke and compare them like any byte. Setting a once-event's count back
    /// to 0 lets it fire again; an answer byte other than 0 and 1 passes both "happened with
    /// Yes" and "happened with No", as in the original.
    #[test]
    fn opcodes_reach_the_runtime_state_bytes() {
        let mut once = global();
        once.results.gold = 5;
        // A many-event that sets event 1's times fired (byte 160) back to 0.
        let reset = many(op(2, -1, 160, 0));
        let mut g = engine(vec![once, reset]);
        let mut w = MockWorld::new();
        assert_eq!(tick_at(&mut g, &mut w, 0), vec![1, 2]);
        assert_eq!(g.times_fired(1), 0);
        assert_eq!(tick_at(&mut g, &mut w, 10), vec![1, 2], "fired again");

        // Times fired compared (op 4: byte 160 = 1), and the answer byte poked to 2.
        let mut both = global();
        let c = &mut both.conditions;
        (c.happened_yes_check, c.happened_yes, c.happened_no_check, c.happened_no) = (1, [1, 0], 1, [1, 0]);
        let mut no = global();
        let c = &mut no.conditions;
        (c.happened_no_check, c.happened_no) = (1, [1, 0]);
        let events = vec![global(), op(4, -1, 160, 1), op(2, -2, 162, 2), both, no];
        let mut g = engine(events);
        let mut w = MockWorld::new();
        assert_eq!(tick_at(&mut g, &mut w, 0), vec![1, 2, 3, 4, 5]);
        assert_eq!(g.happened(1), Some(Answer::Yes));
    }

    /// Opcodes 3–5 compare a byte (c26754): 3 below, 4 equal, 5 above, signed; the second
    /// compare (byte 11 = 3–5) runs for any opcode (c268b4).
    #[test]
    fn opcodes_3_to_5_compare_bytes_of_other_events() {
        let mut second = op(7, 0, 0, 0);
        (second.conditions.squad_count, second.conditions.gold, second.conditions.level, second.conditions.holiness_mana) = (5, -7, 85, 6);
        let mut signed = target_event();
        signed.results.gold = 200;
        let events = vec![
            target_event(),
            op(3, -1, 85, 6), // 5 < 6: yes
            op(3, -2, 85, 5), // no
            op(4, -3, 85, 5), // yes
            op(4, -4, 85, 6), // no
            op(5, -5, 85, 4), // yes
            op(5, -6, 85, 5), // no
            second,           // second setting: 5 > 6: no
            op(3, 40, 85, 0), // no such event: ignored, fires
            signed,           // 10: gold 200, a byte of −56
            op(3, -1, 85, 0), // −56 < 0: yes
        ];
        let mut g = engine(events);
        let mut w = MockWorld::new();
        assert_eq!(tick_at(&mut g, &mut w, 0), vec![2, 4, 6, 9, 11]);
    }

    #[test]
    fn opcodes_6_to_20_act_through_the_world() {
        let mut equip = op(6, 3, 1, 0);
        equip.results.artifacts_add = [7, 0, 9, 0];
        let mut spells = op(11, 2, -1, 0);
        spells.results.spells_learned = [4, 5, 0, 0];
        let mut forget = op(16, 0, 0, 0);
        forget.results.spells_learned = [3, 0, 0, 0];
        let events = vec![
            equip,
            op(6, -2, 0, 0),
            op(7, 0, 4, 12),
            op(8, 0, 1, 0),
            op(8, 5, 8, 0),
            op(9, -1, 4, 0),
            op(10, 2, 0, -3),
            spells,
            op(12, 4, 7, 2),
            op(13, 2, -1, 2000),
            forget,
            op(17, 9, 12, 0),
            op(19, 6, 19, 11),
            op(20, 30, 10, 0),
        ];
        let n = events.len() as u16;
        let mut g = engine(events);
        g.set_named_units(vec![0, 42]);
        let mut w = MockWorld::new();
        assert_eq!(tick_at(&mut g, &mut w, 0), (1..=n).collect::<Vec<_>>());
        use Holder::*;
        assert_eq!(
            w.log,
            vec![
                Fx::Equip(Army(3), 1, [7, 0, 9, 0]),
                Fx::Equip(Building(2), 0, [0; 4]),
                Fx::Replace(Player, 4, 12),
                Fx::Speed(Player, 5),
                Fx::Speed(Army(5), -3),
                Fx::Faction(Building(1), 4),
                Fx::Relation(Army(2), 0, -3),
                Fx::SetSpells(Army(2), None, vec![4, 5, 0, 0]),
                Fx::Named(Army(4), 7, 2, 42),
                Fx::UnitXp(Army(2), None, 2000),
                Fx::Forget(3),
                Fx::Model(Army(9), 12),
                Fx::Target(6, 19, 11),
                Fx::Teleport(30, 10),
            ],
            "no items given, spells learned, XP or gold"
        );
    }

    #[test]
    fn opcode_8_speed_codes() {
        assert_eq!([1, 5, 6, 8, 0].map(speed_correction), [5, 1, -1, -3, 0]);
    }

    #[test]
    fn opcode_14_checks_lasting_spells() {
        let mut check = op(14, 2, -1, 0);
        check.results.spells_learned = [4, 5, 0, 0];
        let mut g = engine(vec![many(check)]);
        let mut w = MockWorld::new();
        w.spells_on.insert(Holder::Army(2), vec![4]);
        assert!(tick_at(&mut g, &mut w, 0).is_empty(), "spell 5 missing");
        w.spells_on.insert(Holder::Army(2), vec![5, 1, 4]);
        assert_eq!(tick_at(&mut g, &mut w, 10), vec![1], "order does not matter");
        assert!(w.log.is_empty(), "no spells learned");
    }

    #[test]
    fn opcode_15_records_the_campaign_branch() {
        let mut branch = op(15, 3, 2, 0);
        branch.results.chained_event = 2;
        let mut win = global();
        win.subordinate = 1;
        let mut g = EventEngine::from_parts(vec![branch, win], Vec::new(), 2, 0);
        let mut w = MockWorld::new();
        let out = g.tick(&mut w);
        assert!(out.contains(&EventOutcome::Victory(2)));
        assert_eq!(g.campaign_branch(), Some((3, 2)));
    }

    /// Opcode 18 (c28710): the digit of this event's `+X^` is a random raw byte between the
    /// low bytes of the XP and gold fields.
    #[test]
    fn opcode_18_draws_the_digit_of_a_counter_flag() {
        let roll = many(titled(op(18, 49, 57, 0), "+Roll^"));
        let wants = titled(global(), "=Roll2");
        let mut g = engine(vec![roll, wants]);
        let mut w = MockWorld::new();
        w.rolls = vec![53, 50];
        assert_eq!(tick_at(&mut g, &mut w, 0), vec![1]);
        assert_eq!(g.flag_string(), "Roll5\u{a0}");
        assert_eq!(tick_at(&mut g, &mut w, DAY), vec![1, 2], "drawn anew: Roll2");
        assert_eq!(g.flag_string(), "Roll2\u{a0}");
    }

    /// The cell test (c2875e) replaces the gold, mana, squad and strength tests of an event
    /// with "current stats", "no meeting" 1, a patrol value up to 19 and an army in the
    /// strength's low byte.
    #[test]
    fn opcode_19_checks_an_army_position_and_sets_its_target() {
        let mut e = many(op(19, 6, 19, 11));
        (e.conditions.stats_check, e.conditions.army_strength, e.conditions.gold, e.conditions.holiness_mana) = (1, 6, 30, 10);
        let mut g = engine(vec![e]);
        let mut w = MockWorld::new();
        w.army_cells.insert(6, (29, 10));
        assert!(tick_at(&mut g, &mut w, 0).is_empty());
        w.army_cells.insert(6, (30, 10));
        assert_eq!(tick_at(&mut g, &mut w, 10), vec![1]);
        assert_eq!(w.log, vec![Fx::Target(6, 19, 11)]);
    }

    /// Opcode 21 (c2a9a2): a named character with the hero must be of the slot's type, one
    /// off (the type is compared without the −1 shift); a missing character passes.
    #[test]
    fn opcode_21_checks_a_named_characters_type() {
        let mut e = many(op(21, 0, 0, 0));
        let c = &mut e.conditions;
        (c.units, c.units_named) = ([73, 0, 0], [2, 0, 0]);
        let mut g = engine(vec![e]);
        let mut w = MockWorld::new();
        assert_eq!(tick_at(&mut g, &mut w, 0), vec![1], "not with the hero");
        w.units.push(unit(73, 2));
        assert!(tick_at(&mut g, &mut w, 10).is_empty(), "type 73 is 72 to the original");
        w.units[1].unit = 74;
        assert_eq!(tick_at(&mut g, &mut w, 20), vec![1]);
    }
}

#[cfg(test)]
mod real_maps {
    //! Every shipped map through a simulated timeline; skipped without `RAZDOR_DT_DIR`.
    use super::mock::MockWorld;
    use super::*;
    use crate::dt::install::DtInstall;

    struct Run {
        engine: EventEngine,
        questions: usize,
        rumours: usize,
        loop_guards: usize,
    }

    /// A player who visits every building and point in turn (one every two hours), beats one
    /// active army a day, meets every active army, hears every rumour and answers questions
    /// Yes and No in turn.
    fn play(s: &Scenario, days: u64) -> Run {
        let mut g = EventEngine::new(s);
        let mut w =
            MockWorld { archetype: 1, level: 5, gold: 1000, mana: 100, squads: 6, strength: 3000, ..MockWorld::new() };
        w.active.extend(s.armies.iter().filter(|a| a.is_active()).map(|a| a.id));
        w.home.extend(s.armies.iter().map(|a| a.id));
        for (i, b) in s.buildings.iter().enumerate() {
            if let Some(o) = b.owner() {
                w.building_owner.insert(i as u16 + 1, o);
            }
        }
        let mut places: Vec<Place> = (1..=s.buildings.len() as u16).map(Place::Building).collect();
        places.extend(s.points.iter().map(|p| Place::Point(p.id)));
        let mut run = Run { engine: g.clone(), questions: 0, rumours: 0, loop_guards: 0 };
        let start = s.header.start_time as u64;
        let mut yes = true;
        'days: for day in 0..days {
            if let Some(a) = w.active.iter().copied().filter(|a| !w.defeated.contains(a)).min() {
                w.defeated.insert(a);
                w.beaten.insert(a);
                w.active.remove(&a);
            }
            for slot in 0..12 {
                w.now = start + day * 1440 + slot * 120;
                w.place = places.get((day * 12 + slot) as usize).copied();
                // Every window is read (OK), every question answered.
                fn settle(g: &mut EventEngine, w: &mut MockWorld, out: &mut Vec<EventOutcome>, yes: &mut bool, questions: &mut usize) {
                    for _ in 0..100 {
                        if g.holds_window() {
                            out.extend(g.window_closed(w));
                            continue;
                        }
                        if g.pending_question().is_none() {
                            break;
                        }
                        *questions += 1;
                        out.extend(g.answer(w, *yes));
                        *yes = !*yes;
                    }
                }
                let mut out = g.tick(&mut w);
                settle(&mut g, &mut w, &mut out, &mut yes, &mut run.questions);
                for a in w.active.clone() {
                    if g.pending_question().is_none() {
                        out.extend(g.meet(&mut w, a));
                        settle(&mut g, &mut w, &mut out, &mut yes, &mut run.questions);
                    }
                }
                for r in g.hall(&w) {
                    run.rumours += 1;
                    out.extend(g.take(&mut w, r));
                    settle(&mut g, &mut w, &mut out, &mut yes, &mut run.questions);
                }
                run.loop_guards += out.iter().filter(|o| **o == EventOutcome::LoopGuard).count();
                if g.ended().is_some() {
                    break 'days;
                }
            }
        }
        run.engine = g;
        run
    }

    #[test]
    fn every_shipped_map_runs_a_simulated_timeline() {
        let Some(dir) = std::env::var_os(crate::dt::install::ENV_VAR) else { return };
        let dt = DtInstall::load(std::path::Path::new(&dir)).expect("install loads");
        let mut extensions = 0;
        for m in &dt.maps {
            let s = m.load().unwrap();
            let run = play(&s, 60);
            let g = &run.engine;
            extensions += g.extensions().len();
            let fired = (1..=s.events.len() as u16).filter(|id| g.times_fired(*id) > 0).count();
            let quests = g.journal().len() + g.completed_quests().len();
            println!(
                "{}: {} events, {fired} fired ({} firings), {} questions, {} rumours, {quests} quests \
                 ({} completed), {} loop guards, ended {:?}, {} Community extensions",
                m.name,
                s.events.len(),
                g.total_fired(),
                run.questions,
                run.rumours,
                g.completed_quests().len(),
                run.loop_guards,
                g.ended(),
                g.extensions().len()
            );
            if m.name.starts_with("РК1") {
                assert!(fired >= 10, "{fired}");
                assert!(quests > 0);
            }
        }
        println!("Community extensions in all maps: {extensions}");
    }
}
