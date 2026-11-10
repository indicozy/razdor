//! The chronicle («Летопись»): a dated record of what the player saw and did through a
//! campaign, kept in the save (Razdor's own feature; the design is
//! `docs/superpowers/specs/2026-11-10-chronicle-design.md`).
//!
//! [`Game`] appends to it where the player sees or does a thing: a text window shown, the
//! player's battles, captures, first visits, hirings, deaths, level-ups, spells, artefacts,
//! quests, the noon report, and each hero step's end cell. The hooks only append, so the
//! rules, the AI and the difftests are unchanged. Nothing under the fog and no fight between
//! AI armies is recorded.
//!
//! Texts are stored by reference where the game has an id (a scenario event by its id, a
//! catalog key with its arguments) and as a string only when built on the fly, so the save
//! stays small after hundreds of days. Chapters follow the journal's: one per campaign map,
//! carried over at the hand-over (`Game::apply_carry_over`).

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::battle::{Battle, Team};
use super::content::{ItemId, UnitId};
use super::events::EventOutcome;
use super::game::{BattleResult, DayReport, Foe, Game};
use super::map::Tile;
use super::world::{EventId, LocationKind};

/// Minutes in a game day (the clock's day).
pub const MINUTES_PER_DAY: u64 = 24 * 60;

/// The size the record's JSON must stay under after 300 days (the save is compressed on top;
/// the tests check it, `replay.rs` on a real campaign and the synthetic record here).
pub const SIZE_BUDGET_300_DAYS: usize = 1_000_000;

/// A text the record shows, by reference where the game has an id.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextRef {
    /// A scenario event's own text (its message or question), `#HERONAME` filled when shown.
    Event(EventId),
    /// A line of the game's catalog (its key) with its arguments, filled when shown.
    Catalog { key: String, args: Vec<String> },
    /// A text built on the fly, kept as it was shown.
    Text(String),
}

/// A battle's foe, by the scenario's id (its name is the scenario's, filled when shown).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FoeRef {
    /// A garrison: the building's scenario id (`Location::id`).
    Garrison(u16),
    /// A scenario army (`Army::id`).
    Army(u8),
}

/// What an entry records.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EntryKind {
    /// A text window was shown; `answer` is the question's answer (`true`: Yes) once given.
    Text { text: TextRef, answer: Option<bool> },
    /// A battle of the player's: where, against whom, the result and the losses on both sides.
    Battle { at: Tile, foe: Option<FoeRef>, won: bool, own_losses: u32, foe_losses: u32 },
    /// A castle, fort, village or ruins the player took (its scenario id).
    Captured { location: u16 },
    /// A building entered for the first time in this chapter (its scenario id).
    FirstVisit { location: u16 },
    /// Units the player hired (merged with the same unit's entries of the same minute).
    Hired { unit: UnitId, count: u32 },
    /// Units the player dismissed.
    Dismissed { unit: UnitId, count: u32 },
    /// Units of the hero's army that fell outside battle (spells, potions, events).
    Fell { unit: UnitId, count: u32 },
    /// The hero reached a new level.
    LevelUp { level: i32 },
    /// The hero learnt a spell (the sanctuary, a scenario event).
    SpellLearned { spell: u32 },
    /// An artefact the hero got: found (a battle's loot, a scenario event) or bought.
    ArtefactFound { item: ItemId },
    ArtefactBought { item: ItemId },
    /// A quest was taken, or completed (by its event's id).
    QuestTaken { quest: EventId },
    QuestDone { quest: EventId },
    /// The noon report: the gold when it opened, the income and the wages shown.
    Noon { gold: i32, income: i32, wages: i32 },
    /// The chronicle starts at a load of a save made before it existed.
    EarlierDaysNotRecorded,
}

impl EntryKind {
    /// Adds `other`'s count to this entry when both are the same unit's hirings, dismissals or
    /// fallings; false when they cannot be merged.
    fn merge(&mut self, other: &EntryKind) -> bool {
        match (self, other) {
            (EntryKind::Hired { unit: a, count: n }, EntryKind::Hired { unit: b, count: m })
            | (EntryKind::Dismissed { unit: a, count: n }, EntryKind::Dismissed { unit: b, count: m })
            | (EntryKind::Fell { unit: a, count: n }, EntryKind::Fell { unit: b, count: m })
                if a == b =>
            {
                *n = n.saturating_add(*m);
                true
            }
            _ => false,
        }
    }
}

/// One line of the record: the game minute it happened at and what it was.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub at_minutes: u64,
    pub kind: EntryKind,
}

/// One campaign map's record.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chapter {
    /// The map's title, as the journal names the chapter.
    pub title: String,
    /// The game minute the chapter began.
    pub from_minutes: u64,
    /// Oldest first.
    pub entries: Vec<Entry>,
    /// The hero's route: the cell at each step's end, without repeats in a row.
    pub route: Vec<Tile>,
    /// The buildings entered in this chapter (scenario ids), for the first visits.
    #[serde(default)]
    pub visited: BTreeSet<u16>,
    /// The route's new cells per day (the day's index from the game's start, `minutes /
    /// MINUTES_PER_DAY`): the summary's longest walk.
    #[serde(default)]
    pub walk_days: BTreeMap<u64, u32>,
}

impl Chapter {
    fn new(title: String, from_minutes: u64) -> Chapter {
        Chapter { title, from_minutes, entries: Vec::new(), route: Vec::new(), visited: BTreeSet::new(), walk_days: BTreeMap::new() }
    }
}

/// The whole record, chapter by chapter. Empty in saves made before it existed (the load then
/// starts it: [`Chronicle::begin_after_load`]).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chronicle {
    #[serde(default)]
    pub chapters: Vec<Chapter>,
}

/// The totals of the record (the «Итоги» table).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Totals {
    /// Game days from the chronicle's start to the given minute (the first day counts as 1).
    pub days: u64,
    pub battles_won: u32,
    pub battles_lost: u32,
    /// Units the player lost in battle, and the foes' units beaten.
    pub own_losses: u32,
    pub foe_losses: u32,
    /// Castles, forts, villages and ruins taken.
    pub towns_taken: u32,
    /// Artefacts found or bought.
    pub artefacts: u32,
    /// Quests completed.
    pub quests_done: u32,
}

/// Appends `cell` to `route` unless the route already ends on it (a step that does not move
/// the hero, or a cell repeated in a row, is not a new line of the route). True when it was
/// appended.
pub fn route_push(route: &mut Vec<Tile>, cell: Tile) -> bool {
    if route.last() != Some(&cell) {
        route.push(cell);
        true
    } else {
        false
    }
}

/// The dates of a record: the game day of `at` counted from `first` (the first day is 1).
pub fn day_of(first: u64, at: u64) -> u64 {
    at.saturating_sub(first) / MINUTES_PER_DAY + 1
}

/// The totals of `chapters` up to minute `now` (the first chapter's start is day 1).
fn totals_of(chapters: &[Chapter], now: u64) -> Totals {
    let mut t = Totals::default();
    let Some(first) = chapters.first() else { return t };
    // The days run to the later of `now` and the record's last entry (a record never ends before it).
    let last = chapters.iter().flat_map(|c| c.entries.iter()).map(|e| e.at_minutes).max().unwrap_or(0);
    t.days = now.max(last).saturating_sub(first.from_minutes) / MINUTES_PER_DAY + 1;
    for e in chapters.iter().flat_map(|c| &c.entries) {
        match e.kind {
            EntryKind::Battle { won, own_losses, foe_losses, .. } => {
                if won {
                    t.battles_won += 1;
                } else {
                    t.battles_lost += 1;
                }
                t.own_losses = t.own_losses.saturating_add(own_losses);
                t.foe_losses = t.foe_losses.saturating_add(foe_losses);
            }
            EntryKind::Captured { .. } => t.towns_taken += 1,
            EntryKind::ArtefactFound { .. } | EntryKind::ArtefactBought { .. } => t.artefacts += 1,
            EntryKind::QuestDone { .. } => t.quests_done += 1,
            _ => {}
        }
    }
    t
}

/// The biggest battle of `chapters` (see [`Chronicle::biggest_battle`]).
fn biggest_battle_of(chapters: &[Chapter]) -> Option<&Entry> {
    let mut best: Option<(u32, &Entry)> = None;
    for e in chapters.iter().flat_map(|c| &c.entries) {
        if let EntryKind::Battle { own_losses, foe_losses, .. } = e.kind {
            let total = own_losses.saturating_add(foe_losses);
            if best.is_none_or(|(b, _)| total > b) {
                best = Some((total, e));
            }
        }
    }
    best.map(|(_, e)| e)
}

/// The richest moment of `chapters` (see [`Chronicle::richest_moment`]).
fn richest_of(chapters: &[Chapter]) -> Option<&Entry> {
    let mut best: Option<(i32, &Entry)> = None;
    for e in chapters.iter().flat_map(|c| &c.entries) {
        if let EntryKind::Noon { gold, .. } = e.kind {
            if best.is_none_or(|(b, _)| gold > b) {
                best = Some((gold, e));
            }
        }
    }
    best.map(|(_, e)| e)
}

/// The biggest hiring of `chapters`: the most units taken on in one entry (the earliest on a tie).
fn most_hired_of(chapters: &[Chapter]) -> Option<&Entry> {
    let mut best: Option<(u32, &Entry)> = None;
    for e in chapters.iter().flat_map(|c| &c.entries) {
        if let EntryKind::Hired { count, .. } = e.kind {
            if best.is_none_or(|(b, _)| count > b) {
                best = Some((count, e));
            }
        }
    }
    best.map(|(_, e)| e)
}

/// A mark on the summary's map.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    /// A battle of the player's, at the cell the hero stood on; `won` or lost.
    Battle { cell: Tile, won: bool },
    /// A building taken (its scenario id: the picture finds its cell).
    Capture { location: u16 },
}

/// The facts the summary's highlights name (each one an entry of the record).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Highlights {
    /// The battle with the most units lost on both sides.
    pub biggest_battle: Option<Entry>,
    /// The entry that took on the most units at once.
    pub most_hired: Option<Entry>,
    /// The noon report with the most gold.
    pub richest: Option<Entry>,
    /// The day with the longest walk: its index (`minutes / MINUTES_PER_DAY`, see
    /// [`Chronicle::day_of`]) and the cells walked that day.
    pub longest_walk: Option<(u64, u32)>,
}

/// The «Итоги» page of a map (or of the story so far): what the picture shows, its marks, the
/// highlights and the totals.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Summary {
    /// The map's title (the chapter the picture shows).
    pub title: String,
    /// The map's picture is the chapter at this index; the totals cover the chapters from `from`.
    pub from: usize,
    pub totals: Totals,
    pub highlights: Highlights,
    /// The hero's route on the map (the picture's chapter), oldest cell first.
    pub route: Vec<Tile>,
    pub marks: Vec<Mark>,
    /// The hero fell: the route's end is where he died.
    pub died: bool,
}

impl Chronicle {
    /// Starts a chapter for map `title` at minute `at`.
    pub fn start(&mut self, title: &str, at: u64) {
        self.chapters.push(Chapter::new(title.to_string(), at));
    }

    /// The next campaign map begins: its chapter (the hand-over).
    pub fn next_chapter(&mut self, title: &str, at: u64) {
        self.start(title, at);
    }

    /// Starts the record of a save that had none: an «earlier days» line at the load, so the
    /// reader knows the days before it are not written down. A record that exists is left
    /// alone.
    pub fn begin_after_load(&mut self, title: &str, at: u64) {
        if !self.chapters.is_empty() {
            return;
        }
        self.start(title, at);
        self.record(at, EntryKind::EarlierDaysNotRecorded);
    }

    /// The chapter being played; a record with none gets one with no title (a game that never
    /// began its map).
    fn current(&mut self, at: u64) -> &mut Chapter {
        if self.chapters.is_empty() {
            self.chapters.push(Chapter::new(String::new(), at));
        }
        self.chapters.last_mut().expect("a chapter")
    }

    /// Appends an entry to the chapter being played. The same unit's hirings, dismissals or
    /// fallings in the same minute are one entry with the count added.
    pub fn record(&mut self, at: u64, kind: EntryKind) {
        let chapter = self.current(at);
        if let Some(last) = chapter.entries.last_mut() {
            if last.at_minutes == at && last.kind.merge(&kind) {
                return;
            }
        }
        chapter.entries.push(Entry { at_minutes: at, kind });
    }

    /// The hero's step ended on `cell`.
    pub fn route_step(&mut self, at: u64, cell: Tile) {
        let chapter = self.current(at);
        if route_push(&mut chapter.route, cell) {
            *chapter.walk_days.entry(at / MINUTES_PER_DAY).or_insert(0) += 1;
        }
    }

    /// A building is entered: recorded only the first time in the chapter.
    pub fn note_first_visit(&mut self, at: u64, location: u16) {
        if self.current(at).visited.insert(location) {
            self.record(at, EntryKind::FirstVisit { location });
        }
    }

    /// A text window was shown: a message, or a question still open (its answer comes with
    /// [`Chronicle::note_answer`]).
    pub fn note_text(&mut self, at: u64, event: EventId) {
        self.record(at, EntryKind::Text { text: TextRef::Event(event), answer: None });
    }

    /// The player answered `event`'s question: its open entry gets the answer. Without one (a
    /// question of a save made before the record), the answer is entered as its own line.
    pub fn note_answer(&mut self, at: u64, event: EventId, yes: bool) {
        let open = self.current(at).entries.iter_mut().rev().find(|e| {
            matches!(&e.kind, EntryKind::Text { text: TextRef::Event(id), answer: None } if *id == event)
        });
        match open {
            Some(Entry { kind: EntryKind::Text { answer, .. }, .. }) => *answer = Some(yes),
            _ => self.record(at, EntryKind::Text { text: TextRef::Event(event), answer: Some(yes) }),
        }
    }

    /// Quest `quest` was completed. A closing event that fires again, while the quest is still
    /// closed, adds nothing (as the journal's, `History::completed_last`).
    pub fn note_quest_done(&mut self, at: u64, quest: EventId) {
        let closed = self.current(at).entries.iter().rev().find_map(|e| match e.kind {
            EntryKind::QuestTaken { quest: q } if q == quest => Some(false),
            EntryKind::QuestDone { quest: q } if q == quest => Some(true),
            _ => None,
        });
        if closed != Some(true) {
            self.record(at, EntryKind::QuestDone { quest });
        }
    }

    /// Every entry of the record, oldest chapter first.
    pub fn entries(&self) -> impl Iterator<Item = &Entry> {
        self.chapters.iter().flat_map(|c| &c.entries)
    }

    /// The record's totals up to minute `now`.
    pub fn totals(&self, now: u64) -> Totals {
        totals_of(&self.chapters, now)
    }

    /// The biggest battle: the most units lost on both sides (the earliest on a tie).
    pub fn biggest_battle(&self) -> Option<&Entry> {
        biggest_battle_of(&self.chapters)
    }

    /// The richest moment: the most gold in a noon report (the earliest on a tie).
    pub fn richest_moment(&self) -> Option<&Entry> {
        richest_of(&self.chapters)
    }

    /// The texts the record holds by reference, kept in full where `text_of` still knows them
    /// (its argument: whether the window was a question). Made at the hand-over, when the
    /// finished map's script is gone; the save then holds the texts of the maps before.
    pub fn with_texts(&self, text_of: impl Fn(EventId, bool) -> Option<String>) -> Chronicle {
        let mut c = self.clone();
        for e in c.chapters.iter_mut().flat_map(|c| c.entries.iter_mut()) {
            if let EntryKind::Text { text, answer } = &mut e.kind {
                let asked = answer.is_some();
                if let TextRef::Event(id) = *text {
                    if let Some(full) = text_of(id, asked) {
                        *text = TextRef::Text(full);
                    }
                }
            }
        }
        c
    }

    /// The day number of minute `at` in this record (the first chapter's day is 1).
    pub fn day_of(&self, at: u64) -> u64 {
        day_of(self.chapters.first().map_or(at, |c| c.from_minutes), at)
    }

    /// The «Итоги» of the map being played (the last chapter's picture: its route, its battles and
    /// captures) with the totals and highlights of the chapters from `from` on. `died`: the hero
    /// fell. None with no chapter.
    pub fn summary(&self, from: usize, now: u64, died: bool) -> Option<Summary> {
        let last = self.chapters.last()?;
        let covered = &self.chapters[from.min(self.chapters.len() - 1)..];
        let marks = last
            .entries
            .iter()
            .filter_map(|e| match e.kind {
                EntryKind::Battle { at, won, .. } => Some(Mark::Battle { cell: at, won }),
                EntryKind::Captured { location } => Some(Mark::Capture { location }),
                _ => None,
            })
            .collect();
        let longest_walk = covered
            .iter()
            .flat_map(|c| c.walk_days.iter())
            .fold(None::<(u64, u32)>, |best, (&day, &steps)| match best {
                Some((_, s)) if s >= steps => best,
                _ => Some((day, steps)),
            });
        Some(Summary {
            title: last.title.clone(),
            from: from.min(self.chapters.len() - 1),
            totals: totals_of(covered, now),
            highlights: Highlights {
                biggest_battle: biggest_battle_of(covered).cloned(),
                most_hired: most_hired_of(covered).cloned(),
                richest: richest_of(covered).cloned(),
                longest_walk,
            },
            route: last.route.clone(),
            marks,
            died,
        })
    }
}

/// The hooks: what [`Game`] calls where the player sees or does a thing (the list is in
/// `docs/flows/campaign-scenario.md`, «Chronicle»). Each one only appends to
/// [`Game::chronicle`]; none changes the game.
impl Game {
    fn chronicle_now(&self) -> u64 {
        self.clock.total_minutes() as u64
    }

    /// The hero's step ended on his cell (`Game::run_pause`).
    pub(crate) fn chronicle_step(&mut self) {
        let (at, cell) = (self.chronicle_now(), self.tile());
        self.chronicle.route_step(at, cell);
    }

    /// Building `l` was taken by the player (`Game::begin_walk`, `Game::settle_battle`).
    pub(crate) fn chronicle_capture(&mut self, l: usize) {
        let Some(loc) = self.world.locations.get(l) else { return };
        let (at, location) = (self.chronicle_now(), loc.id);
        self.chronicle.record(at, EntryKind::Captured { location });
    }

    /// The hero entered building `l` (`Game::enter_building`): a bridge or an obelisk opens
    /// nothing, so it is not a visit.
    pub(crate) fn chronicle_first_visit(&mut self, l: usize) {
        let Some(loc) = self.world.locations.get(l) else { return };
        if loc.kind.is_bridge() || loc.kind == LocationKind::Obelisk {
            return;
        }
        let (at, location) = (self.chronicle_now(), loc.id);
        self.chronicle.note_first_visit(at, location);
    }

    /// A unit was hired (`Game::hire`).
    pub(crate) fn chronicle_hired(&mut self, unit: UnitId) {
        let at = self.chronicle_now();
        self.chronicle.record(at, EntryKind::Hired { unit, count: 1 });
    }

    /// A unit was sent away (`Game::dismiss`).
    pub(crate) fn chronicle_dismissed(&mut self, unit: UnitId) {
        let at = self.chronicle_now();
        self.chronicle.record(at, EntryKind::Dismissed { unit, count: 1 });
    }

    /// Who of the hero's army is alive, for [`Game::chronicle_fallen`].
    pub(crate) fn alive_mask(&self) -> Vec<bool> {
        self.squad.iter().map(|u| u.alive()).collect()
    }

    /// The units alive in `before` that are dead now fell outside battle: a map spell or a
    /// potion (`Game::apply_spell_to_army_ext`, `Game::drink`). Battles record their losses in
    /// their own entry.
    pub(crate) fn chronicle_fallen(&mut self, before: &[bool]) {
        let fallen: Vec<UnitId> = self.squad.iter().zip(before).filter(|(u, was)| **was && !u.alive()).map(|(u, _)| u.def).collect();
        let at = self.chronicle_now();
        for unit in fallen {
            self.chronicle.record(at, EntryKind::Fell { unit, count: 1 });
        }
    }

    /// Squad member `k` reached `level`: the hero's (squad 0) are recorded.
    pub(crate) fn chronicle_level_up(&mut self, k: usize, level: i32) {
        if k == 0 {
            let at = self.chronicle_now();
            self.chronicle.record(at, EntryKind::LevelUp { level });
        }
    }

    /// The hero learnt spell `spell` (the sanctuary or a scenario event).
    pub(crate) fn chronicle_spell(&mut self, spell: u32) {
        let at = self.chronicle_now();
        self.chronicle.record(at, EntryKind::SpellLearned { spell });
    }

    /// The hero got artefact `item`: bought, or found.
    pub(crate) fn chronicle_artefact(&mut self, item: ItemId, bought: bool) {
        let at = self.chronicle_now();
        let kind = if bought { EntryKind::ArtefactBought { item } } else { EntryKind::ArtefactFound { item } };
        self.chronicle.record(at, kind);
    }

    /// The text windows and the quest lines of the engine's outcomes (`Game::script_events`,
    /// before the journal takes them). Silent events and the rest are not shown, so not kept.
    pub(crate) fn chronicle_outcomes(&mut self, out: &[EventOutcome]) {
        let at = self.chronicle_now();
        for o in out {
            match *o {
                EventOutcome::Fired { event, message: true } => self.chronicle.note_text(at, event),
                EventOutcome::Question(event) => self.chronicle.note_text(at, event),
                EventOutcome::QuestAdded(quest) => self.chronicle.record(at, EntryKind::QuestTaken { quest }),
                EventOutcome::QuestCompleted { quest, .. } => self.chronicle.note_quest_done(at, quest),
                _ => {}
            }
        }
    }

    /// The player answered `event`'s question (`Game::answer_question`).
    pub(crate) fn chronicle_answer(&mut self, event: EventId, yes: bool) {
        let at = self.chronicle_now();
        self.chronicle.note_answer(at, event, yes);
    }

    /// The noon report was shown (`Game::new_day`).
    pub(crate) fn chronicle_noon(&mut self, r: &DayReport) {
        let at = self.chronicle_now();
        self.chronicle.record(at, EntryKind::Noon { gold: r.gold, income: r.income, wages: r.wages });
    }

    /// The foe of a battle by its scenario id, read before the battle is settled (settling can
    /// take the army off the map).
    pub(crate) fn chronicle_foe(&self, foe: Option<Foe>) -> Option<FoeRef> {
        match foe? {
            Foe::Garrison(l) => self.world.locations.get(l).map(|loc| FoeRef::Garrison(loc.id)),
            Foe::Army(i) => self.world.armies.get(i).map(|a| FoeRef::Army(a.id)),
        }
    }

    /// The place and the foe of a battle, read before it is settled (`Game::resolve_battle`).
    pub(crate) fn chronicle_battle_start(&self) -> (Tile, Option<FoeRef>) {
        (self.tile(), self.chronicle_foe(self.foe))
    }

    /// A battle of the player's ended (`Game::resolve_battle`): its entry, then the captures,
    /// the hero's level-ups and the loot the result reports.
    pub(crate) fn chronicle_battle(&mut self, start: (Tile, Option<FoeRef>), battle: &Battle, result: &BattleResult) {
        let at = self.chronicle_now();
        let own_losses = battle.fighters.iter().filter(|f| f.team == Team::Player && f.hp <= 0).count() as u32;
        let foe_losses = battle.fighters.iter().filter(|f| f.team == Team::Enemy && f.hp <= 0).count() as u32;
        let won = matches!(result, BattleResult::Victory { .. });
        let (cell, foe) = start;
        self.chronicle.record(at, EntryKind::Battle { at: cell, foe, won, own_losses, foe_losses });
        if let BattleResult::Victory { loot, level_ups, captured, .. } = result {
            if let Some(l) = *captured {
                self.chronicle_capture(l);
            }
            for &(k, level) in level_ups {
                self.chronicle_level_up(k, level);
            }
            for &item in loot {
                self.chronicle_artefact(item, false);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn battle(won: bool, own: u32, foe: u32) -> EntryKind {
        EntryKind::Battle { at: (1, 1), foe: Some(FoeRef::Army(2)), won, own_losses: own, foe_losses: foe }
    }

    #[test]
    fn route_push_drops_a_repeat_in_a_row_only() {
        let mut r = Vec::new();
        for c in [(1, 1), (1, 1), (2, 1), (2, 1), (1, 1)] {
            route_push(&mut r, c);
        }
        assert_eq!(r, vec![(1, 1), (2, 1), (1, 1)]);
    }

    #[test]
    fn counts_of_the_same_unit_in_one_minute_merge() {
        let mut c = Chronicle::default();
        c.start("M", 0);
        c.record(5, EntryKind::Hired { unit: UnitId(4), count: 1 });
        c.record(5, EntryKind::Hired { unit: UnitId(4), count: 2 });
        c.record(5, EntryKind::Hired { unit: UnitId(5), count: 1 });
        c.record(6, EntryKind::Hired { unit: UnitId(5), count: 1 });
        let e = &c.chapters[0].entries;
        assert_eq!(e.len(), 3);
        assert_eq!(e[0].kind, EntryKind::Hired { unit: UnitId(4), count: 3 });
    }

    #[test]
    fn first_visit_is_recorded_once_per_chapter() {
        let mut c = Chronicle::default();
        c.start("M", 0);
        c.note_first_visit(1, 7);
        c.note_first_visit(9, 7);
        c.next_chapter("N", 20);
        c.note_first_visit(21, 7);
        let n: usize = c.chapters.iter().map(|c| c.entries.len()).sum();
        assert_eq!(n, 2, "once in each chapter");
    }

    #[test]
    fn an_answer_lands_on_the_open_question_entry() {
        let mut c = Chronicle::default();
        c.start("M", 0);
        c.note_text(3, 9);
        c.note_answer(3, 9, false);
        assert_eq!(c.chapters[0].entries[0].kind, EntryKind::Text { text: TextRef::Event(9), answer: Some(false) });
        c.note_answer(4, 9, true);
        assert_eq!(c.chapters[0].entries.len(), 2, "no open question: the answer is its own line");
    }

    #[test]
    fn quest_done_is_recorded_once_while_closed() {
        let mut c = Chronicle::default();
        c.start("M", 0);
        c.record(1, EntryKind::QuestTaken { quest: 3 });
        c.note_quest_done(2, 3);
        c.note_quest_done(2, 3);
        c.record(4, EntryKind::QuestTaken { quest: 3 });
        c.note_quest_done(5, 3);
        let kinds: Vec<_> = c.chapters[0].entries.iter().map(|e| &e.kind).collect();
        assert_eq!(kinds.iter().filter(|k| matches!(k, EntryKind::QuestDone { .. })).count(), 2);
    }

    #[test]
    fn totals_count_battles_losses_and_captures() {
        let mut c = Chronicle::default();
        c.start("M", 0);
        c.record(10, battle(true, 3, 7));
        c.record(20, battle(false, 5, 1));
        c.record(30, EntryKind::Captured { location: 4 });
        let t = c.totals(MINUTES_PER_DAY * 2 + 5);
        assert_eq!(t.days, 3);
        assert_eq!((t.battles_won, t.battles_lost), (1, 1));
        assert_eq!((t.own_losses, t.foe_losses, t.towns_taken), (8, 8, 1));
    }

    #[test]
    fn highlights_pick_the_biggest_battle_and_the_richest_noon() {
        let mut c = Chronicle::default();
        c.start("M", 0);
        c.record(10, battle(true, 2, 2));
        c.record(20, battle(true, 4, 3));
        c.record(30, battle(false, 1, 1));
        c.record(40, EntryKind::Noon { gold: 500, income: 0, wages: 0 });
        c.record(50, EntryKind::Noon { gold: 900, income: 0, wages: 0 });
        c.record(60, EntryKind::Noon { gold: 900, income: 0, wages: 0 });
        assert_eq!(c.biggest_battle().map(|e| e.at_minutes), Some(20));
        assert_eq!(c.richest_moment().map(|e| e.at_minutes), Some(50));
    }

    #[test]
    fn an_old_save_gets_the_earlier_days_line_once() {
        let mut c = Chronicle::default();
        c.begin_after_load("M", 100);
        c.begin_after_load("M", 200);
        assert_eq!(c.chapters.len(), 1);
        assert_eq!(c.chapters[0].entries, vec![Entry { at_minutes: 100, kind: EntryKind::EarlierDaysNotRecorded }]);
    }

    #[test]
    fn walks_count_each_new_cell_once_per_day() {
        let mut c = Chronicle::default();
        c.start("M", 0);
        c.route_step(5, (1, 1));
        c.route_step(6, (1, 1));
        c.route_step(7, (2, 1));
        c.route_step(MINUTES_PER_DAY + 1, (3, 1));
        let ch = &c.chapters[0];
        assert_eq!(ch.route, vec![(1, 1), (2, 1), (3, 1)]);
        assert_eq!(ch.walk_days.get(&0), Some(&2));
        assert_eq!(ch.walk_days.get(&1), Some(&1));
        let s = c.summary(0, MINUTES_PER_DAY * 2, false).expect("a chapter");
        assert_eq!(s.highlights.longest_walk, Some((0, 2)), "the earliest day with the most cells");
        assert_eq!(c.day_of(MINUTES_PER_DAY * 2 + 5), 3);
    }

    #[test]
    fn the_summary_totals_cover_the_chapters_from_the_given_one_and_the_picture_is_the_last() {
        let mut c = Chronicle::default();
        c.start("First", 0);
        c.record(10, battle(true, 3, 7));
        c.record(20, EntryKind::Captured { location: 4 });
        c.next_chapter("Second", MINUTES_PER_DAY);
        c.route_step(MINUTES_PER_DAY + 1, (5, 5));
        c.record(MINUTES_PER_DAY + 2, EntryKind::Battle { at: (5, 5), foe: None, won: false, own_losses: 4, foe_losses: 1 });
        c.record(MINUTES_PER_DAY + 3, EntryKind::QuestDone { quest: 2 });
        let now = MINUTES_PER_DAY + 10;
        let last = c.summary(1, now, true).expect("a summary");
        assert_eq!(last.title, "Second");
        assert_eq!((last.totals.battles_won, last.totals.battles_lost), (0, 1), "only the second map's battles");
        assert_eq!(last.totals.quests_done, 1);
        assert_eq!(last.marks, vec![Mark::Battle { cell: (5, 5), won: false }], "the picture's marks: its own map's");
        assert_eq!(last.route, vec![(5, 5)]);
        assert!(last.died);
        let story = c.summary(0, now, false).expect("a summary");
        assert_eq!((story.totals.battles_won, story.totals.battles_lost), (1, 1), "the story so far counts both");
        assert_eq!(story.totals.towns_taken, 1);
        assert_eq!(story.totals.days, 2);
        assert_eq!(story.from, 0);
        assert!(c.summary(0, now, false).is_some() && Chronicle::default().summary(0, now, false).is_none());
    }

    #[test]
    fn the_most_hired_is_the_largest_single_hiring() {
        let mut c = Chronicle::default();
        c.start("M", 0);
        c.record(1, EntryKind::Hired { unit: UnitId(4), count: 2 });
        c.record(50, EntryKind::Hired { unit: UnitId(5), count: 6 });
        c.record(90, EntryKind::Hired { unit: UnitId(4), count: 3 });
        let h = c.summary(0, 100, false).expect("a summary").highlights;
        assert_eq!(h.most_hired.map(|e| e.at_minutes), Some(50));
        assert!(h.biggest_battle.is_none() && h.richest.is_none() && h.longest_walk.is_none());
    }

    #[test]
    fn texts_are_kept_in_full_only_where_the_script_still_knows_them() {
        let mut c = Chronicle::default();
        c.start("M", 0);
        c.note_text(1, 7);
        c.note_text(2, 8);
        c.note_answer(2, 8, true);
        let kept = c.with_texts(|id, asked| (id == 7).then(|| format!("text {id} asked={asked}")));
        let kinds: Vec<_> = kept.chapters[0].entries.iter().map(|e| e.kind.clone()).collect();
        assert_eq!(kinds[0], EntryKind::Text { text: TextRef::Text("text 7 asked=false".into()), answer: None });
        assert_eq!(kinds[1], EntryKind::Text { text: TextRef::Event(8), answer: Some(true) });
    }

    #[test]
    fn a_300_day_record_at_a_realistic_rate_stays_under_the_budget() {
        // Per day: 150 route cells (about ten hours of walking), six text windows, a noon report,
        // a battle every third day, a hiring every tenth.
        let mut c = Chronicle::default();
        c.start("Map", 0);
        let mut cell = (40, 40);
        for day in 0..300u64 {
            let t0 = day * MINUTES_PER_DAY;
            for k in 0..150u64 {
                cell = (40 + (k % 20) as i32, 40 + (day % 9) as i32 + (k % 3) as i32);
                c.route_step(t0 + k * 4, cell);
            }
            for w in 0..6u64 {
                c.note_text(t0 + 500 + w, (10 + (day + w) % 40) as EventId);
            }
            c.record(t0 + 720, EntryKind::Noon { gold: 1000 + day as i32, income: 300, wages: 120 });
            if day % 3 == 0 {
                c.record(t0 + 900, EntryKind::Battle { at: cell, foe: Some(FoeRef::Army(3)), won: day % 2 == 0, own_losses: 2, foe_losses: 5 });
            }
            if day % 10 == 0 {
                c.record(t0 + 1000, EntryKind::Hired { unit: UnitId(5), count: 1 });
            }
        }
        let json = serde_json::to_string(&c).unwrap();
        println!("chronicle, 300 days at the realistic rate: {} entries, {} bytes", c.entries().count(), json.len());
        assert!(json.len() < SIZE_BUDGET_300_DAYS, "{} bytes", json.len());
    }
}
