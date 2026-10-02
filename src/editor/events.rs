//! Editing events: new records, the time window as the original editor shows it, signed
//! thresholds, the flag script in the title, the Community opcode view, references to an
//! event, the event list's filter, and event pictures.
//!
//! What the original editor does was read from its forms and its save routine
//! (`docs/reference/dtm-format.md` §9 maps every control to its byte): the repeat period is
//! edited in days (stored × 1440 minutes), the active duration in hours (stored × 60), a
//! "relative only" event has the start [`RELATIVE_START`], the "once" byte is the inverse of
//! its "can happen many times" box, a threshold's ≥/≤ switch is the sign of the stored value,
//! and the title is `name[%(+X|-X)][=X|=/X]`, built from two separate fields.

use crate::dt::dtm::{Event, EventKind, Scenario, RELATIVE_START};
use crate::i18n::{n_, tr};
use crate::trf;
use crate::rules::events::{event_edits, extension, EventEdit, Extension};

/// The original editor keeps at most this many events (its arrays hold 5000).
pub const MAX_EVENTS: usize = 5000;
const DAY: u32 = 1440;
const HOUR: u32 = 60;
/// The repeat period the original offers: every 1–31 days.
pub const MAX_REPEAT_DAYS: u32 = 31;
/// Longest active duration the original's spin offers.
pub const MAX_DURATION_HOURS: u32 = 99;
/// The building types the event window's building pickers offer (0x532654): villages,
/// castles, forts, churches, altars and ruins; no towns.
pub const PICKER_BUILDINGS: [u8; 6] = [2, 3, 4, 7, 10, 12];
/// The extra entry at the end of the condition unit lists.
pub const CONDITION_EXTRA_UNIT: u8 = 0xFF;

/// Event type labels (byte 1), Razdor's own words (translated where shown).
pub const KIND_LABELS: [(u8, &str); 4] = [(1, n_("Global")), (2, n_("Local")), (3, n_("Quest")), (4, n_("Rumour"))];

pub fn kind_label(kind: u8) -> &'static str {
    tr(KIND_LABELS.iter().find(|k| k.0 == kind).map_or(n_("Unknown type"), |k| k.1))
}

/// Owner codes of the ownership conditions (bytes 33, 43, 50): 0 is the empty entry.
pub const OWNERS: [(u8, &str); 7] =
    [(0, n_("(any)")), (1, n_("The player")), (2, n_("Green")), (3, n_("Blue")), (4, n_("Yellow")), (5, n_("Red")), (6, n_("Not the player"))];

/// The archetype radio group (byte 10).
pub const ARCHETYPES: [&str; 4] = [n_("Every hero"), n_("Knight"), n_("Archmage"), n_("Ranger")];

/// Special "units removed" entries (bytes 105–108).
pub const REMOVE_ADDED_UNIT: u8 = 0xFE;
pub const REMOVE_ANY_UNIT: u8 = 0xFF;

/// Pictures (byte 82) besides unit portraits.
pub const PICTURE_DEFEAT: u8 = 200;
pub const PICTURE_VICTORY: u8 = 201;

/// Editor groups (byte 0): six colours.
pub const GROUPS: u8 = 6;

/// A new event as the original's New button prepares it (0x538fe4): a zeroed record of
/// type `kind` with a default title ending in its number, from the scenario's start,
/// repeating every day and open for 1,440 (as stored), "once" the inverse of the editor's
/// "new events repeat" option.
pub fn new_event(s: &Scenario, kind: u8, new_events_repeat: bool) -> Event {
    Event {
        kind,
        start_time: s.header.start_time,
        repeat: DAY as u16,
        duration: DAY as u16,
        once: (!new_events_repeat) as u8,
        title: trf!("Event {n}", n = s.events.len() + 1),
        ..Event::default()
    }
}

/// Why the original's copy stops: a name whose only `#` is its last character (reading the
/// character after it is a range error).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HashAtEnd;

/// The name a copy gets (0x539214): with a `#`, the character after the first `#` is raised
/// by one character code in the map's code page (so a 9 becomes `:`, not 10); without one,
/// ` #1` is appended.
pub fn copy_name(name: &str) -> Result<String, HashAtEnd> {
    let mut bytes = crate::dt::text::encode(name);
    match bytes.iter().position(|b| *b == b'#') {
        None => Ok(format!("{name} #1")),
        Some(k) if k + 1 >= bytes.len() => Err(HashAtEnd),
        Some(k) => {
            bytes[k + 1] = bytes[k + 1].wrapping_add(1);
            Ok(crate::dt::text::decode(&bytes))
        }
    }
}

/// A unit picker and its named-character picker as the event window links them (0x53b2dc,
/// 0x53b39c): picking a character sets the unit to the character's class; changing the
/// unit clears the character. `before` and `after` are (unit, character).
pub fn pick_unit_and_named(s: &Scenario, before: (u8, u8), after: (u8, u8)) -> (u8, u8) {
    if after.1 != before.1 && after.1 != 0 {
        let class = (after.1 as usize).checked_sub(1).and_then(|i| s.named_characters.get(i)).map_or(after.0, |n| n.unit);
        return (class, after.1);
    }
    if after.0 != before.0 {
        return (after.0, 0);
    }
    after
}

// ------------------------------------------------------------------------------------------
// Time window
// ------------------------------------------------------------------------------------------

pub fn is_relative(e: &Event) -> bool {
    e.start_time == RELATIVE_START
}

/// "Relative only" (0x53ac54): the event waits until another event moves its start, and
/// it is not subordinate. Turning it off gives the event the scenario's start `start`.
pub fn set_relative(e: &mut Event, on: bool, start: u32) {
    if on {
        e.start_time = RELATIVE_START;
        e.subordinate = 0;
    } else if is_relative(e) {
        e.start_time = start;
    }
}

/// "Subordinate" (0x53aa10): on, the start, repeat and duration are 0 and the event can
/// happen many times (once 0), and it is not "relative only"; off, they become the scenario's
/// start `start`, one day and 24 hours, and the event happens once.
pub fn set_subordinate(e: &mut Event, on: bool, start: u32) {
    e.subordinate = on as u8;
    if on {
        (e.start_time, e.repeat, e.duration, e.once) = (0, 0, 0, 0);
    } else {
        (e.start_time, e.repeat, e.duration, e.once) = (start, DAY as u16, (24 * HOUR) as u16, 1);
    }
}

/// The repeat period in whole days (0 = no repeat).
pub fn repeat_days(e: &Event) -> u32 {
    e.repeat as u32 / DAY
}

pub fn set_repeat_days(e: &mut Event, days: u32) {
    e.repeat = (days.min(u16::MAX as u32 / DAY) * DAY) as u16;
}

/// The active duration in whole hours.
pub fn duration_hours(e: &Event) -> u32 {
    e.duration as u32 / HOUR
}

pub fn set_duration_hours(e: &mut Event, hours: u32) {
    e.duration = (hours.min(MAX_DURATION_HOURS) * HOUR) as u16;
}

/// "Can happen many times" (the inverse of the "once" byte).
pub fn repeatable(e: &Event) -> bool {
    e.once == 0
}

pub fn set_repeatable(e: &mut Event, on: bool) {
    e.once = (!on) as u8;
}

// ------------------------------------------------------------------------------------------
// Signed thresholds (squad count, strength, level, gold, mana)
// ------------------------------------------------------------------------------------------

/// A condition threshold: `at_least` is the ≥ switch (stored positive), else ≤ (negative).
/// A value of 0 turns the check off.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Threshold {
    pub at_least: bool,
    pub value: u16,
}

impl Threshold {
    pub fn from_raw(v: i16) -> Threshold {
        Threshold { at_least: v >= 0, value: v.unsigned_abs() }
    }

    /// The stored value. The original keeps a signed word: "at least" holds up to 32,767,
    /// "at most" (negated) up to 32,768; a larger value stops its save with a range error,
    /// here it is held at those ends.
    pub fn raw(self) -> i16 {
        if self.at_least {
            self.value.min(i16::MAX as u16) as i16
        } else {
            (-(self.value.min(32_768) as i32)) as i16
        }
    }

    /// The largest value the switch's side can store.
    pub fn max(at_least: bool) -> u16 {
        if at_least {
            i16::MAX as u16
        } else {
            32_768
        }
    }
}

// ------------------------------------------------------------------------------------------
// Title and flag script
// ------------------------------------------------------------------------------------------

/// A title split as the original editor edits it: the name, the "flag" field (`+X` sets,
/// `-X` clears) and the "check flag" field (`X` must be set, `/X` must not).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TitleParts {
    pub name: String,
    pub set: String,
    pub test: String,
}

pub fn split_title(title: &str) -> TitleParts {
    match title.split_once('%') {
        None => TitleParts { name: title.to_string(), ..TitleParts::default() },
        Some((name, script)) => {
            let (set, test) = script.split_once('=').unwrap_or((script, ""));
            TitleParts { name: name.to_string(), set: set.to_string(), test: test.to_string() }
        }
    }
}

/// The title the original editor writes: the name; `%` and the flag field if it starts with
/// `+` or `-` (otherwise the field is dropped); `%` if only a check is given; `=` and the
/// check field.
pub fn compose_title(p: &TitleParts) -> String {
    let mut t = p.name.clone();
    let has_set = p.set.starts_with('+') || p.set.starts_with('-');
    if has_set {
        t.push('%');
        t.push_str(&p.set);
    }
    if !p.test.is_empty() {
        if !has_set {
            t.push('%');
        }
        t.push('=');
        t.push_str(&p.test);
    }
    t
}

/// The title with its name replaced and its flag script kept as it is.
pub fn with_name(title: &str, name: &str) -> String {
    let name: String = name.chars().filter(|c| *c != '%').collect();
    match title.find('%') {
        Some(i) => format!("{name}{}", &title[i..]),
        None => name,
    }
}

/// The title with new flag fields and its name kept.
pub fn with_flags(title: &str, set: &str, test: &str) -> String {
    let name = split_title(title).name;
    compose_title(&TitleParts { name, set: set.to_string(), test: test.to_string() })
}

fn bad_flag_name(n: &str) -> Option<String> {
    if n.is_empty() {
        return Some(tr("a flag name is empty").into());
    }
    if n.contains(['%', '=']) {
        return Some(trf!("the flag name \"{n}\" contains % or =", n));
    }
    if n.chars().any(char::is_whitespace) {
        return Some(trf!("the flag name \"{n}\" contains a space", n));
    }
    None
}

/// What is wrong with the flag script of a title (empty when it is fine or absent).
pub fn flag_problems(title: &str) -> Vec<String> {
    let Some((_, script)) = title.split_once('%') else { return Vec::new() };
    let mut out = Vec::new();
    if script.contains('%') {
        out.push(tr("the title has more than one %").into());
    }
    let (set, test) = match script.split_once('=') {
        Some((s, t)) => (s, Some(t)),
        None => (script, None),
    };
    if !set.is_empty() {
        match set.strip_prefix('+').or_else(|| set.strip_prefix('-')) {
            Some(n) => out.extend(bad_flag_name(n)),
            None => out.push(trf!("the flag action \"{set}\" must start with + (set) or - (clear)", set)),
        }
    }
    if let Some(t) = test {
        out.extend(bad_flag_name(t.strip_prefix('/').unwrap_or(t)));
    }
    if set.is_empty() && test.is_none() {
        out.push(tr("the title ends with % but has no flag script").into());
    }
    out
}

// ------------------------------------------------------------------------------------------
// Community opcodes
// ------------------------------------------------------------------------------------------

/// What an opcode's argument names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arg {
    /// The player's army (0), a scenario army (1–255) or a building (−id).
    Holder,
    /// A scenario army (1–255), or the player (0) where allowed.
    Army,
    /// A relative shift in the event list (opcodes 1–5).
    EventShift,
    /// A byte offset of the event record (opcodes 1–5).
    EventField,
    /// A named character (1-based).
    Named,
    /// A unit id.
    Unit,
    /// Any number.
    Number,
}

/// One Community opcode: its name and what its three arguments (the XP, gold and mana
/// changes of the first results tab) mean. Labels are Razdor's own, after the Community
/// guide (English, translated where shown).
#[derive(Clone, Copy, Debug)]
pub struct OpcodeInfo {
    pub code: u8,
    pub name: &'static str,
    /// (label, kind) of the XP, gold and mana fields; `None` = unused.
    pub args: [Option<(&'static str, Arg)>; 3],
    /// A list of the results tab the opcode uses, if any.
    pub uses: Option<&'static str>,
    /// Checks something instead of doing it.
    pub condition: bool,
}

const fn a(label: &'static str, kind: Arg) -> Option<(&'static str, Arg)> {
    Some((label, kind))
}

const SHIFT: Option<(&str, Arg)> = a(n_("Target event (+ down, - up)"), Arg::EventShift);
const FIELD: Option<(&str, Arg)> = a(n_("Field (byte offset)"), Arg::EventField);
const HOLDER: Option<(&str, Arg)> = a(n_("Army (0 = player, -N = building N)"), Arg::Holder);
const SLOT: Option<(&str, Arg)> = a(n_("Unit slot (0 = leader)"), Arg::Number);
const SLOT_ALL: Option<(&str, Arg)> = a(n_("Unit slot (-1 = every unit)"), Arg::Number);

pub const OPCODES: [OpcodeInfo; 20] = [
    OpcodeInfo { code: 1, name: n_("Edit an event: add"), args: [SHIFT, FIELD, a(n_("Value added"), Arg::Number)], uses: None, condition: false },
    OpcodeInfo { code: 2, name: n_("Edit an event: set"), args: [SHIFT, FIELD, a(n_("Value set"), Arg::Number)], uses: None, condition: false },
    OpcodeInfo { code: 3, name: n_("Check an event: field at most"), args: [SHIFT, FIELD, a(n_("Value"), Arg::Number)], uses: None, condition: true },
    OpcodeInfo { code: 4, name: n_("Check an event: field equal"), args: [SHIFT, FIELD, a(n_("Value"), Arg::Number)], uses: None, condition: true },
    OpcodeInfo { code: 5, name: n_("Check an event: field at least"), args: [SHIFT, FIELD, a(n_("Value"), Arg::Number)], uses: None, condition: true },
    OpcodeInfo { code: 6, name: n_("Equip a unit"), args: [HOLDER, SLOT, None], uses: Some(n_("the four artefacts gained")), condition: false },
    OpcodeInfo { code: 7, name: n_("Replace a unit"), args: [HOLDER, SLOT, a(n_("New unit"), Arg::Unit)], uses: None, condition: false },
    OpcodeInfo { code: 8, name: n_("Set an army's speed"), args: [a(n_("Army (0 = player)"), Arg::Army), a(n_("Speed (1 = +5 ... 8 = -3)"), Arg::Number), None], uses: None, condition: false },
    OpcodeInfo { code: 9, name: n_("Change faction"), args: [HOLDER, a(n_("Faction (1 player ... 4 enemy)"), Arg::Number), None], uses: None, condition: false },
    OpcodeInfo { code: 10, name: n_("Change an attitude"), args: [HOLDER, a(n_("Towards (0 player ... 3 enemy)"), Arg::Number), a(n_("Attitude (-3 to 3)"), Arg::Number)], uses: None, condition: false },
    OpcodeInfo { code: 11, name: n_("Lasting spells"), args: [HOLDER, SLOT_ALL, None], uses: Some(n_("the spells learned")), condition: false },
    OpcodeInfo { code: 12, name: n_("Place a named character"), args: [HOLDER, SLOT, a(n_("Named character"), Arg::Named)], uses: None, condition: false },
    OpcodeInfo { code: 13, name: n_("Give experience"), args: [HOLDER, SLOT_ALL, a(n_("Experience"), Arg::Number)], uses: None, condition: false },
    OpcodeInfo { code: 14, name: n_("Check spells"), args: [a(n_("Army (0 = player)"), Arg::Army), SLOT_ALL, None], uses: Some(n_("the spells learned")), condition: true },
    OpcodeInfo { code: 15, name: n_("Campaign branch"), args: [a(n_("Map number"), Arg::Number), a(n_("Variant"), Arg::Number), None], uses: Some(n_("the chained event (the victory event)")), condition: false },
    OpcodeInfo { code: 16, name: n_("Forget spells"), args: [None, None, None], uses: Some(n_("the spells learned")), condition: false },
    OpcodeInfo { code: 17, name: n_("Change the map figure"), args: [a(n_("Army"), Arg::Army), a(n_("Figure (0-12)"), Arg::Number), None], uses: None, condition: false },
    OpcodeInfo { code: 18, name: n_("Random digit of its counter flag"), args: [a(n_("Lowest character code"), Arg::Number), a(n_("Highest character code"), Arg::Number), None], uses: None, condition: false },
    OpcodeInfo { code: 19, name: n_("AI army target / position check"), args: [a(n_("Army to send (1-255)"), Arg::Army), a(n_("Target X"), Arg::Number), a(n_("Target Y"), Arg::Number)], uses: Some(n_("strength, gold and mana conditions: army and cell to check")), condition: false },
    OpcodeInfo { code: 20, name: n_("Teleport the player"), args: [a("X", Arg::Number), a("Y", Arg::Number), None], uses: None, condition: false },
];

pub fn opcode_info(code: u8) -> Option<&'static OpcodeInfo> {
    OPCODES.iter().find(|o| o.code == code)
}

/// The opcode of an event the editor offers: "no meeting" set and a patrol change of 1–20
/// (the engine reads any other patrol value as an opcode too).
pub fn opcode(e: &Event) -> Option<u8> {
    match extension(e) {
        Some(Extension::Opcode(op @ 1..=20)) => Some(op),
        _ => None,
    }
}

/// Makes the event run `op` (sets "no meeting" and the patrol change), or turns the opcode
/// off (patrol change 0; "no meeting" is kept).
pub fn set_opcode(e: &mut Event, op: Option<u8>) {
    match op {
        Some(op) => {
            e.results.no_meeting = 1;
            e.results.patrol_delta = op.clamp(1, 20) as i8;
        }
        None => {
            if opcode(e).is_some() {
                e.results.patrol_delta = 0;
            }
        }
    }
}

/// The opcode's arguments: the XP, gold and mana changes.
pub fn opcode_args(e: &Event) -> [i16; 3] {
    [e.results.experience, e.results.gold, e.results.mana]
}

pub fn set_opcode_args(e: &mut Event, args: [i16; 3]) {
    [e.results.experience, e.results.gold, e.results.mana] = args;
}

/// The second event-editing setting of opcodes 1–5 (the squad count condition is its
/// action 1–5, gold the shift, level the field, holiness and mana the value).
pub fn second_edit(e: &Event) -> Option<EventEdit> {
    event_edits(e).get(1).copied()
}

/// Sets (or with `None` clears) the second setting.
pub fn set_second_edit(e: &mut Event, edit: Option<EventEdit>) {
    let c = &mut e.conditions;
    match edit {
        Some(x) => {
            c.squad_count = x.action.clamp(1, 5);
            (c.gold, c.level, c.holiness_mana) = (x.shift, x.field, x.value);
        }
        None => {
            if (1..=5).contains(&c.squad_count) {
                c.squad_count = 0;
            }
        }
    }
}

/// The event record's fields by byte offset, for the opcode 1–5 field picker (Razdor's own
/// labels; the offsets are `docs/reference/dtm-format.md` §9).
pub const EVENT_FIELDS: [(u16, &str); 90] = [
    (0, n_("Group")), (1, n_("Event type")), (2, n_("Start time")), (6, n_("Repeat period")), (8, n_("Active duration")), (10, n_("Hero archetype")),
    (11, n_("Squad count")), (13, n_("Army strength")), (15, n_("Army inactive")), (16, n_("Patrol army")), (17, n_("Patrol change")),
    (18, n_("Check current stats")), (19, n_("Level")), (21, n_("Gold (condition)")), (25, n_("Holiness and mana (condition)")),
    (29, n_("Check buildings")), (30, n_("Building 1")), (31, n_("Building 2")), (32, n_("Building 3")), (33, n_("Building 1 owner")), (34, n_("Building 2 owner")), (35, n_("Building 3 owner")),
    (36, n_("Check named squads")), (37, n_("Squad 1 unit")), (38, n_("Squad 2 unit")), (39, n_("Squad 3 unit")), (40, n_("Squad 1 name")), (41, n_("Squad 2 name")), (42, n_("Squad 3 name")),
    (43, n_("Squad 1 owner")), (44, n_("Squad 2 owner")), (45, n_("Squad 3 owner")),
    (46, n_("Check artefacts")), (47, n_("Artefact 1")), (48, n_("Artefact 2")), (49, n_("Artefact 3")), (50, n_("Artefact 1 owner")), (51, n_("Artefact 2 owner")), (52, n_("Artefact 3 owner")),
    (53, n_("Check defeated armies")), (54, n_("Defeated army 1")), (55, n_("Defeated army 2")),
    (56, n_("Check happened (yes)")), (57, n_("Happened (yes) 1")), (59, n_("Happened (yes) 2")),
    (61, n_("Check not happened")), (62, n_("Not happened 1")), (64, n_("Not happened 2")),
    (66, n_("Check beaten armies")), (67, n_("Beaten army 1")), (68, n_("Beaten army 2")),
    (69, n_("Check happened (no)")), (70, n_("Happened (no) 1")), (72, n_("Happened (no) 2")),
    (74, n_("Meet army")), (75, n_("Army active")), (76, n_("Ask a question")),
    (77, n_("Relative event")), (79, n_("Relative delay (hours)")), (81, n_("Spell on the player")), (82, n_("Picture")),
    (83, n_("Experience change")), (85, n_("Gold change")), (89, n_("Mana change")),
    (93, n_("Spell learned 1")), (97, n_("Unit added 1")), (101, n_("Added unit 1 name")), (105, n_("Unit removed 1")), (109, n_("Removed unit 1 name")),
    (113, n_("Artefact gained 1")), (117, n_("Artefact lost 1")), (121, n_("Army activated 1")), (122, n_("Army activated 2")), (123, n_("Army deactivated")),
    (124, n_("Quest completed")), (126, n_("Delay (hours)")), (128, n_("Lantern 1")), (136, n_("Removed units go to")), (137, n_("New hero class")),
    (138, n_("Chained event")), (140, n_("Subordinate")), (141, n_("Once")), (142, n_("Added units come from")), (143, n_("Move to the hero")),
    (144, n_("Show army")), (145, n_("Hero has 1 HP")), (146, n_("Army at home")), (147, n_("Battle with army")), (148, n_("No meeting")), (149, n_("Repeat after yes")),
];

pub fn event_field_label(off: i16) -> String {
    EVENT_FIELDS.iter().find(|f| f.0 as i16 == off).map_or(trf!("Byte {off}", off), |f| format!("{} ({off})", tr(f.1)))
}

// ------------------------------------------------------------------------------------------
// References to an event
// ------------------------------------------------------------------------------------------

/// Where an event is referred to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RefSite {
    /// A condition or result of another event (or itself).
    Event { id: u16, what: &'static str },
    /// A building's local list.
    Building(u16),
    /// A point's list.
    Point(u8),
    Victory,
    Defeat,
}

impl std::fmt::Display for RefSite {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RefSite::Event { id, what } => f.write_str(&trf!("event {id} ({what})", id, what = tr(what))),
            RefSite::Building(id) => f.write_str(&trf!("building {id}", id)),
            RefSite::Point(id) => f.write_str(&trf!("point {id}", id)),
            RefSite::Victory => f.write_str(tr("the victory event")),
            RefSite::Defeat => f.write_str(tr("the defeat event")),
        }
    }
}

/// The event ids an event refers to, with what each is (the Community "edit event" targets
/// included).
pub fn event_links(id: u16, e: &Event) -> Vec<(u16, &'static str)> {
    let (c, r) = (&e.conditions, &e.results);
    let mut out = Vec::new();
    for x in c.happened_yes {
        out.push((x, n_("happened with yes")));
    }
    for x in c.happened_no {
        out.push((x, n_("happened with no")));
    }
    for x in c.not_happened {
        out.push((x, n_("not happened")));
    }
    out.push((r.relative_event, n_("relative event")));
    out.push((r.completes_quest, n_("quest completed")));
    out.push((r.chained_event, n_("chained event")));
    for edit in event_edits(e) {
        let target = id as i64 + edit.shift as i64;
        if edit.shift != 0 && (1..=u16::MAX as i64).contains(&target) {
            out.push((target as u16, n_("edited by its opcode")));
        }
    }
    out.retain(|x| x.0 != 0);
    out
}

/// Everything that refers to event `id`.
pub fn references_to(s: &Scenario, id: u16) -> Vec<RefSite> {
    let mut out = Vec::new();
    for (i, e) in s.events.iter().enumerate() {
        let from = i as u16 + 1;
        for (x, what) in event_links(from, e) {
            if x == id {
                out.push(RefSite::Event { id: from, what });
            }
        }
    }
    for (i, b) in s.buildings.iter().enumerate() {
        if b.events().any(|x| x == id) {
            out.push(RefSite::Building(i as u16 + 1));
        }
    }
    for p in &s.points {
        if p.events().any(|x| x == id) {
            out.push(RefSite::Point(p.id));
        }
    }
    if s.header.victory_event == id {
        out.push(RefSite::Victory);
    }
    if s.header.defeat_event == id {
        out.push(RefSite::Defeat);
    }
    out
}

/// The buildings and points whose lists hold event `id` (1-based positions).
pub fn places_of(s: &Scenario, id: u16) -> (Vec<u16>, Vec<u16>) {
    let buildings = s.buildings.iter().enumerate().filter(|(_, b)| b.events().any(|x| x == id)).map(|(i, _)| i as u16 + 1).collect();
    let points = s.points.iter().enumerate().filter(|(_, p)| p.events().any(|x| x == id)).map(|(i, _)| i as u16 + 1).collect();
    (buildings, points)
}

// ------------------------------------------------------------------------------------------
// The event list
// ------------------------------------------------------------------------------------------

/// The list's filter: a type, a group colour, a search in the titles.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EventFilter {
    pub kind: Option<u8>,
    pub group: Option<u8>,
    pub search: String,
}

impl EventFilter {
    pub fn matches(&self, e: &Event) -> bool {
        if self.kind.is_some_and(|k| k != e.kind) || self.group.is_some_and(|g| g != e.group_colour) {
            return false;
        }
        let q = self.search.trim().to_lowercase();
        q.is_empty() || e.title_text().to_lowercase().contains(&q)
    }
}

/// The ids of the events that pass the filter, in id order.
pub fn filtered(s: &Scenario, f: &EventFilter) -> Vec<u16> {
    s.events.iter().enumerate().filter(|(_, e)| f.matches(e)).map(|(i, _)| i as u16 + 1).collect()
}

/// "#3 Title" (the title without its flag script).
pub fn event_label(s: &Scenario, id: u16) -> String {
    match s.event(id) {
        Some(e) if !e.title_text().trim().is_empty() => format!("#{id} {}", e.title_text().trim()),
        Some(_) => format!("#{id}"),
        None => format!("#{id} {}", tr("(missing)")),
    }
}

/// Kinds of the ids a checked result or condition refers to must have.
pub fn is_quest(s: &Scenario, id: u16) -> bool {
    s.event(id).and_then(Event::kind) == Some(EventKind::Quest)
}

// ------------------------------------------------------------------------------------------
// Pictures
// ------------------------------------------------------------------------------------------

/// Side of an event picture (the shipped one is 128 × 128).
pub const PICTURE_SIDE: u16 = 128;

/// An event picture from RGBA pixels: scaled (nearest) to 128 × 128 and stored as the map
/// keeps it: u16 width, u16 height, then 16-bit RGB565 pixels, little-endian.
pub fn picture_from_rgba(width: u32, height: u32, rgba: &[u8]) -> Option<Vec<u8>> {
    if width == 0 || height == 0 || rgba.len() < (width * height * 4) as usize {
        return None;
    }
    let side = PICTURE_SIDE as u32;
    let mut out = Vec::with_capacity(4 + (side * side * 2) as usize);
    out.extend(PICTURE_SIDE.to_le_bytes());
    out.extend(PICTURE_SIDE.to_le_bytes());
    for y in 0..side {
        for x in 0..side {
            let (sx, sy) = (x * width / side, y * height / side);
            let i = ((sy * width + sx) * 4) as usize;
            let (r, g, b) = (rgba[i] as u16, rgba[i + 1] as u16, rgba[i + 2] as u16);
            let v = ((r >> 3) << 11) | ((g >> 2) << 5) | (b >> 3);
            out.extend(v.to_le_bytes());
        }
    }
    Some(out)
}

/// The size a stored picture declares, if its data is complete.
pub fn picture_size(data: &[u8]) -> Option<(u16, u16)> {
    let w = u16::from_le_bytes([*data.first()?, *data.get(1)?]);
    let h = u16::from_le_bytes([*data.get(2)?, *data.get(3)?]);
    (data.len() == 4 + 2 * w as usize * h as usize).then_some((w, h))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_fields_in_the_original_units() {
        let mut e = Event::default();
        set_repeat_days(&mut e, 3);
        set_duration_hours(&mut e, 24);
        assert_eq!((e.repeat, e.duration), (4320, 1440));
        assert_eq!((repeat_days(&e), duration_hours(&e)), (3, 24));
        set_repeat_days(&mut e, 100);
        assert_eq!(e.repeat, 45 * 1440, "clamped to what u16 minutes hold");
        set_duration_hours(&mut e, 5000);
        assert_eq!(duration_hours(&e), 99, "the original's spin stops at 99 hours");
        e.start_time = 777;
        set_relative(&mut e, true, 5);
        assert!(is_relative(&e));
        assert_eq!(e.start_time, 1_036_800_000);
        set_relative(&mut e, false, 5);
        assert_eq!(e.start_time, 5);
        set_relative(&mut e, false, 9);
        assert_eq!(e.start_time, 5, "an event with its own start keeps it");
        set_repeatable(&mut e, true);
        assert_eq!(e.once, 0);
        set_repeatable(&mut e, false);
        assert!(!repeatable(&e) && e.once == 1);
        // Subordinate and relative-only exclude each other.
        set_subordinate(&mut e, true, 5);
        assert_eq!((e.subordinate, e.start_time, e.repeat, e.duration, e.once), (1, 0, 0, 0, 0));
        set_relative(&mut e, true, 5);
        assert_eq!((e.subordinate, e.start_time), (0, RELATIVE_START));
        set_subordinate(&mut e, false, 5);
        assert_eq!((e.subordinate, e.start_time, e.repeat, e.duration, e.once), (0, 5, 1440, 1440, 1));
    }

    #[test]
    fn named_characters_follow_their_class() {
        let s = Scenario { named_characters: vec![crate::dt::dtm::NamedCharacter { unit: 12, name: "Эд".into() }], ..Scenario::default() };
        assert_eq!(pick_unit_and_named(&s, (5, 0), (5, 1)), (12, 1), "a character brings its class");
        assert_eq!(pick_unit_and_named(&s, (12, 1), (7, 1)), (7, 0), "a new unit clears the character");
        assert_eq!(pick_unit_and_named(&s, (12, 1), (12, 0)), (12, 0));
    }

    #[test]
    fn copies_are_numbered_as_the_original() {
        assert_eq!(copy_name("Встреча").unwrap(), "Встреча #1");
        assert_eq!(copy_name("Встреча #1").unwrap(), "Встреча #2");
        assert_eq!(copy_name("Встреча #9").unwrap(), "Встреча #:", "one character code up, not 10");
        assert_eq!(copy_name("A#1 B#5").unwrap(), "A#2 B#5", "the first #");
        assert_eq!(copy_name("Сон#а").unwrap(), "Сон#б", "in the map's code page");
        assert_eq!(copy_name("Сон #"), Err(HashAtEnd));
    }

    #[test]
    fn thresholds_keep_the_sign() {
        assert_eq!(Threshold::from_raw(-5), Threshold { at_least: false, value: 5 });
        assert_eq!(Threshold::from_raw(7).raw(), 7);
        assert_eq!(Threshold { at_least: false, value: 12 }.raw(), -12);
        assert_eq!(Threshold { at_least: true, value: 40000 }.raw(), i16::MAX);
        assert_eq!(Threshold { at_least: false, value: 32768 }.raw(), i16::MIN, "at most stores 32,768");
        assert_eq!(Threshold { at_least: false, value: 40000 }.raw(), i16::MIN);
        assert_eq!((Threshold::max(true), Threshold::max(false)), (32767, 32768));
        assert_eq!(Threshold::from_raw(i16::MIN).value, 32768);
    }

    #[test]
    fn titles_split_and_compose_as_the_original() {
        for t in ["Plain", "Quest%+Foo", "Q%-Foo=Foo", "Q%=/Foo", "Q%+A=/B", "Встреча%+Знак=Дорога"] {
            assert_eq!(compose_title(&split_title(t)), t, "{t}");
        }
        let p = split_title("Q%-Foo=/Bar");
        assert_eq!((p.name.as_str(), p.set.as_str(), p.test.as_str()), ("Q", "-Foo", "/Bar"));
        // A flag field without + or - is dropped, as the original does.
        assert_eq!(compose_title(&TitleParts { name: "N".into(), set: "Foo".into(), test: String::new() }), "N");
        assert_eq!(compose_title(&TitleParts { name: "N".into(), set: "Foo".into(), test: "X".into() }), "N%=X");
        assert_eq!(with_name("Old%+F=G", "New"), "New%+F=G");
        assert_eq!(with_name("Old", "A%B"), "AB");
        assert_eq!(with_flags("Name%+F", "", "/G"), "Name%=/G");
        assert_eq!(with_flags("Name%+F", "", ""), "Name");
        // The engine reads what we write.
        let f = crate::dt::dtm::FlagScript::from_title(&with_flags("N", "-Ключ", "/Дверь")).unwrap();
        assert_eq!((f.clear.as_deref(), f.require_unset.as_deref()), (Some("Ключ"), Some("Дверь")));
    }

    #[test]
    fn flag_syntax_problems() {
        assert!(flag_problems("Plain").is_empty());
        assert!(flag_problems("Q%+Foo=/Bar").is_empty());
        assert!(flag_problems("Q%=Bar").is_empty());
        assert_eq!(flag_problems("Q%Foo").len(), 1);
        assert!(flag_problems("Q%+").iter().any(|m| m.contains("empty")));
        assert!(flag_problems("Q%+A=").iter().any(|m| m.contains("empty")));
        assert!(flag_problems("Q%+A B").iter().any(|m| m.contains("space")));
        assert!(flag_problems("Q%+A%B").iter().any(|m| m.contains("more than one")));
        assert!(!flag_problems("Q%").is_empty());
    }

    #[test]
    fn opcode_fields_round_trip() {
        let mut e = Event::default();
        assert_eq!(opcode(&e), None);
        set_opcode(&mut e, Some(13));
        set_opcode_args(&mut e, [2, -1, 2000]);
        assert_eq!((e.results.no_meeting, e.results.patrol_delta), (1, 13));
        assert_eq!((e.results.experience, e.results.gold, e.results.mana), (2, -1, 2000));
        assert_eq!((opcode(&e), opcode_args(&e)), (Some(13), [2, -1, 2000]));
        // The engine sees the same opcode.
        assert_eq!(extension(&e), Some(Extension::Opcode(13)));
        set_opcode(&mut e, None);
        assert_eq!((opcode(&e), e.results.no_meeting), (None, 1));
        // A "no meeting" event without a patrol value is not an opcode, and turning an
        // opcode off there changes nothing.
        let mut plain = Event::default();
        plain.results.no_meeting = 1;
        plain.results.patrol_delta = -5;
        set_opcode(&mut plain, None);
        assert_eq!(plain.results.patrol_delta, -5);
        // Opcodes 1–5 have a second setting in the conditions.
        let mut ed = Event::default();
        set_opcode(&mut ed, Some(2));
        set_opcode_args(&mut ed, [-7, 85, 3]);
        set_second_edit(&mut ed, Some(EventEdit { action: 3, shift: 2, field: 83, value: 10 }));
        assert_eq!((ed.conditions.squad_count, ed.conditions.gold, ed.conditions.level, ed.conditions.holiness_mana), (3, 2, 83, 10));
        let edits = event_edits(&ed);
        assert_eq!(edits, vec![EventEdit { action: 2, shift: -7, field: 85, value: 3 }, EventEdit { action: 3, shift: 2, field: 83, value: 10 }]);
        assert_eq!(second_edit(&ed), Some(edits[1]));
        set_second_edit(&mut ed, None);
        assert_eq!((ed.conditions.squad_count, second_edit(&ed)), (0, None));
        assert!(OPCODES.iter().enumerate().all(|(i, o)| o.code as usize == i + 1));
        assert_eq!(event_field_label(85), "Gold change (85)");
        // Every listed field is one the engine can edit.
        for (off, _) in EVENT_FIELDS {
            assert!(crate::rules::events::event_field(&Event::default(), off).is_some(), "{off}");
        }
    }

    #[test]
    fn filter_and_labels() {
        let s = Scenario {
            events: vec![
                Event { kind: 1, group_colour: 2, title: "Дорога домой%+A".into(), ..Event::default() },
                Event { kind: 3, title: "Find the sword".into(), ..Event::default() },
                Event { kind: 3, group_colour: 2, title: "ДОРОГА".into(), ..Event::default() },
            ],
            ..Scenario::default()
        };
        assert_eq!(filtered(&s, &EventFilter::default()), [1, 2, 3]);
        assert_eq!(filtered(&s, &EventFilter { kind: Some(3), ..Default::default() }), [2, 3]);
        assert_eq!(filtered(&s, &EventFilter { group: Some(2), ..Default::default() }), [1, 3]);
        assert_eq!(filtered(&s, &EventFilter { search: "дорога".into(), ..Default::default() }), [1, 3]);
        assert_eq!(filtered(&s, &EventFilter { search: "+A".into(), ..Default::default() }), Vec::<u16>::new(), "flag scripts are not searched");
        assert_eq!(event_label(&s, 1), "#1 Дорога домой");
        assert_eq!(event_label(&s, 9), "#9 (missing)");
        assert!(is_quest(&s, 2) && !is_quest(&s, 1));
    }

    #[test]
    fn references() {
        let mut s = Scenario { events: vec![Event::default(); 4], ..Scenario::default() };
        s.events[0].conditions.happened_yes = [2, 0];
        s.events[1].results.chained_event = 2;
        s.events[2].results.completes_quest = 2;
        // Opcode 1 in event 4 edits the event two up (event 2).
        set_opcode(&mut s.events[3], Some(1));
        set_opcode_args(&mut s.events[3], [-2, 85, 1]);
        s.buildings = vec![Default::default(), Default::default()];
        s.buildings[1].event_count = 1;
        s.buildings[1].event_slots[0] = 2;
        s.points = vec![crate::dt::dtm::Point { id: 1, event_count: 2, event_slots: [3, 2, 0, 0, 0, 0, 0, 0, 0, 0], ..Default::default() }];
        s.header.victory_event = 2;
        let r = references_to(&s, 2);
        assert_eq!(
            r,
            vec![
                RefSite::Event { id: 1, what: "happened with yes" },
                RefSite::Event { id: 2, what: "chained event" },
                RefSite::Event { id: 3, what: "quest completed" },
                RefSite::Event { id: 4, what: "edited by its opcode" },
                RefSite::Building(2),
                RefSite::Point(1),
                RefSite::Victory,
            ]
        );
        assert_eq!(r[0].to_string(), "event 1 (happened with yes)");
        assert_eq!(places_of(&s, 2), (vec![2], vec![1]));
        assert!(references_to(&s, 1).is_empty());
    }

    #[test]
    fn pictures_are_rgb565() {
        // 2 × 1: red, blue; scaled to 128 × 128 the left half is red.
        let rgba = [255, 0, 0, 255, 0, 0, 255, 255];
        let p = picture_from_rgba(2, 1, &rgba).unwrap();
        assert_eq!(p.len(), 4 + 128 * 128 * 2);
        assert_eq!(picture_size(&p), Some((128, 128)));
        assert_eq!(&p[4..6], &0xF800u16.to_le_bytes());
        assert_eq!(&p[4 + 127 * 2..4 + 128 * 2], &0x001Fu16.to_le_bytes());
        assert!(picture_from_rgba(0, 1, &[]).is_none());
        assert!(picture_size(&p[..10]).is_none());
    }

    #[test]
    fn new_events() {
        let mut s = Scenario::default();
        s.header.start_time = 12345;
        s.events = vec![Event::default(); 4];
        let e = new_event(&s, 3, true);
        assert_eq!((e.kind, e.start_time, e.repeat, e.duration, e.once, e.title.as_str()), (3, 12345, 1440, 1440, 0, "Event 5"));
        assert_eq!(new_event(&s, 1, false).once, 1);
    }
}
