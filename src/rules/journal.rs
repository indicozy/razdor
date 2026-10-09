//! The journal's history: everything the player has learnt, with the in-game date (a Razdor
//! extra the players asked for; the original's journal lists only the quests).
//!
//! [`Game`] records here, as the event engine reports it: quests received and completed,
//! rumours heard, and the messages of scripted events (silent ones are skipped). Texts are
//! kept as the scenario wrote them (`#HERONAME` unfilled, so a name chosen after the opening
//! events still shows) and filled when shown. They come from the player's own install and
//! live only in his saves.
//!
//! Campaigns: the history carries over to the next map; each map is a new *chapter*, so the
//! event numbers of two maps never mix.

use serde::{Deserialize, Serialize};

use super::clock::Clock;
use super::events::{EventId, EventOutcome};
use super::game::Game;
use crate::dt::dtm::EventKind;
use crate::i18n::tr;

/// What a journal entry records.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EntryKind {
    /// A quest was added to the journal.
    Quest,
    /// A quest was completed.
    Completed,
    /// A rumour heard in a main hall.
    Rumour,
    /// A scripted event's message.
    Message,
}

/// One thing the player learnt.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub kind: EntryKind,
    pub event: EventId,
    /// The campaign map it came from (0: the first one played).
    pub chapter: u32,
    /// In-game minutes when it happened ([`Clock::at_minutes`]).
    pub minutes: u64,
    /// The event's title and text as the scenario has them (escapes unfilled).
    pub title: String,
    pub text: String,
}

impl Entry {
    pub fn date(&self) -> Clock {
        Clock::at_minutes(self.minutes)
    }
}

/// The history, oldest first.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct History {
    pub entries: Vec<Entry>,
    /// The chapter being played.
    #[serde(default)]
    pub chapter: u32,
    /// The map title of each chapter, by number (empty in saves made before they were kept).
    #[serde(default)]
    pub titles: Vec<String>,
}

impl History {
    /// Adds an entry of the current chapter. The same event reported twice in the same
    /// minute with the same text is kept once.
    pub fn record(&mut self, kind: EntryKind, event: EventId, minutes: u64, title: &str, text: &str) {
        let e = Entry { kind, event, chapter: self.chapter, minutes, title: title.to_string(), text: text.to_string() };
        if self.entries.iter().rev().take_while(|x| x.minutes == minutes).any(|x| *x == e) {
            return;
        }
        self.entries.push(e);
    }

    /// Entries of `kind`, newest first.
    pub fn newest(&self, kind: EntryKind) -> impl Iterator<Item = &Entry> {
        self.entries.iter().rev().filter(move |e| e.kind == kind)
    }

    /// The latest entry of `kind` for event `id` of the current chapter.
    pub fn find(&self, kind: EntryKind, id: EventId) -> Option<&Entry> {
        self.newest(kind).find(|e| e.event == id && e.chapter == self.chapter)
    }

    /// The next campaign map begins.
    pub fn next_chapter(&mut self) {
        self.chapter += 1;
    }

    /// Names the chapter being played after its map.
    pub fn name_chapter(&mut self, title: &str) {
        let n = self.chapter as usize;
        if self.titles.len() <= n {
            self.titles.resize(n + 1, String::new());
        }
        self.titles[n] = title.trim().to_string();
    }

    /// The heading of chapter `n` in the journal: its map's title, else its number.
    pub fn chapter_heading(&self, n: u32) -> String {
        match self.titles.get(n as usize).filter(|t| !t.is_empty()) {
            Some(t) => t.clone(),
            None => crate::trf!("Chapter {n}", n = n + 1),
        }
    }
}

/// The journal screen's tabs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Active,
    Completed,
    Rumours,
    Messages,
}

impl Tab {
    pub const ALL: [Tab; 4] = [Tab::Active, Tab::Completed, Tab::Rumours, Tab::Messages];

    pub fn label(self) -> &'static str {
        match self {
            Tab::Active => tr("Active quests"),
            Tab::Completed => tr("Completed"),
            Tab::Rumours => tr("Rumours"),
            Tab::Messages => tr("Messages"),
        }
    }
}

/// A line of a tab, texts filled in for showing.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub title: String,
    pub text: String,
    /// When it happened; `None` for quests from a save made before the history existed.
    pub date: Option<Clock>,
    /// An active quest: minutes since its event last fired, as the original's journal shows
    /// it (0x49c388: no deadline, the time since the latest firing).
    pub elapsed: Option<u64>,
    /// The campaign map it belongs to ([`Entry::chapter`]).
    pub chapter: u32,
}

impl Game {
    /// An event's title as shown: before its flag script and its editor's note (`#`),
    /// escapes filled in; "Event" when it has none.
    pub fn event_title(&self, id: EventId) -> String {
        let raw = self.script().and_then(|s| s.event(id)).map_or("", |e| e.display_title().trim());
        if raw.is_empty() {
            tr("Event").to_string()
        } else {
            self.fill_text(raw)
        }
    }

    /// The raw (title, message) of event `id`.
    fn event_texts(&self, id: EventId) -> (String, String) {
        self.script()
            .and_then(|s| s.event(id))
            .map_or_else(Default::default, |e| (e.display_title().trim().to_string(), e.message.clone()))
    }

    /// A line of the quest journal as the original fills its detail (0x49c388): the title,
    /// then the question (if any) and the message as one text, without `#HERONAME` filled
    /// (the stored text keeps it), and the time since the event last fired.
    fn quest_row(&self, id: EventId) -> Row {
        let Some((script, e)) = self.script().and_then(|s| Some((s, s.event(id)?))) else {
            return Row { title: tr("Event").to_string(), text: String::new(), date: None, elapsed: None, chapter: self.journal.chapter };
        };
        let title = e.display_title().trim();
        let text = if e.question.is_empty() { e.message.clone() } else { format!("{}\n{}", e.question, e.message) };
        let now = self.clock.total_minutes() as u64;
        Row {
            title: if title.is_empty() { tr("Event").to_string() } else { title.to_string() },
            text: text.replace('\r', ""),
            date: self.journal.find(EntryKind::Quest, id).map(Entry::date),
            elapsed: script.last_fired(id).map(|l| now.saturating_sub(l)),
            chapter: self.journal.chapter,
        }
    }

    /// Notes what the event engine reported in the history (see the module notes).
    pub(crate) fn record_outcomes(&mut self, out: &[EventOutcome]) {
        let minutes = self.clock.total_minutes() as u64;
        for o in out {
            let (kind, id) = match *o {
                EventOutcome::QuestAdded(id) => (EntryKind::Quest, id),
                EventOutcome::QuestCompleted(id) => (EntryKind::Completed, id),
                EventOutcome::Fired { event, message: true } => {
                    match self.script().and_then(|s| s.event(event)).and_then(|e| e.kind()) {
                        // A quest's text is its journal entry.
                        Some(EventKind::Quest) => continue,
                        Some(EventKind::Rumour) => (EntryKind::Rumour, event),
                        _ => (EntryKind::Message, event),
                    }
                }
                _ => continue,
            };
            let (title, text) = self.event_texts(id);
            if matches!(kind, EntryKind::Rumour | EntryKind::Message) && text.trim().is_empty() {
                continue;
            }
            self.journal.record(kind, id, minutes, &title, &text);
        }
    }

    /// The lines of a journal tab, newest first. Active quests are the engine's journal (with
    /// their date from the history); completed ones the history's, then any the engine lists
    /// that the history lacks (older saves).
    pub fn journal_rows(&self, tab: Tab) -> Vec<Row> {
        let shown = |e: &Entry| Row { title: self.fill_title(&e.title), text: self.fill_text(&e.text), date: Some(e.date()), elapsed: None, chapter: e.chapter };
        let from_engine = |id: EventId, kind: EntryKind| match self.journal.find(kind, id) {
            Some(e) => shown(e),
            None => {
                let (_, text) = self.event_texts(id);
                Row { title: self.event_title(id), text: self.fill_text(&text), date: None, elapsed: None, chapter: self.journal.chapter }
            }
        };
        let (active, done) = self.script().map_or((&[][..], &[][..]), |s| (s.journal(), s.completed_quests()));
        match tab {
            // The engine's journal, an entry each time a quest was received.
            Tab::Active => active.iter().rev().map(|&id| self.quest_row(id)).collect(),
            Tab::Completed => {
                let mut rows: Vec<Row> = self.journal.newest(EntryKind::Completed).map(shown).collect();
                let missing = done.iter().rev().filter(|&&id| self.journal.find(EntryKind::Completed, id).is_none());
                rows.extend(missing.map(|&id| from_engine(id, EntryKind::Quest)));
                rows
            }
            Tab::Rumours => self.journal.newest(EntryKind::Rumour).map(shown).collect(),
            Tab::Messages => self.journal.newest(EntryKind::Message).map(shown).collect(),
        }
    }

    fn fill_title(&self, raw: &str) -> String {
        if raw.trim().is_empty() {
            tr("Event").to_string()
        } else {
            self.fill_text(raw.trim())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_report_in_the_same_minute_is_kept_once() {
        let mut h = History::default();
        h.record(EntryKind::Message, 3, 10, "t", "x");
        h.record(EntryKind::Message, 3, 10, "t", "x");
        h.record(EntryKind::Message, 3, 11, "t", "x");
        assert_eq!(h.entries.len(), 2, "a later repeat is a new entry");
        h.record(EntryKind::Rumour, 3, 11, "t", "x");
        assert_eq!(h.entries.len(), 3);
    }

    #[test]
    fn newest_first_and_chapters_apart() {
        let mut h = History::default();
        h.record(EntryKind::Quest, 1, 5, "a", "");
        h.record(EntryKind::Quest, 2, 6, "b", "");
        let titles: Vec<&str> = h.newest(EntryKind::Quest).map(|e| e.title.as_str()).collect();
        assert_eq!(titles, ["b", "a"]);
        h.next_chapter();
        assert!(h.find(EntryKind::Quest, 1).is_none(), "event 1 of the last map is another event");
        h.record(EntryKind::Quest, 1, 9, "c", "");
        assert_eq!(h.find(EntryKind::Quest, 1).map(|e| e.title.as_str()), Some("c"));
    }

    #[test]
    fn chapters_are_headed_by_their_maps() {
        let mut h = History::default();
        h.name_chapter(" РК1-Начало пути ");
        h.next_chapter();
        h.next_chapter();
        h.name_chapter("РК3");
        assert_eq!(h.chapter_heading(0), "РК1-Начало пути");
        assert_eq!(h.chapter_heading(2), "РК3");
        assert_ne!(h.chapter_heading(1), "", "a chapter of an older save is headed by its number");
    }

    #[test]
    fn old_saves_have_no_history() {
        let h: History = serde_json::from_str("{\"entries\": []}").unwrap();
        assert_eq!(h, History::default());
    }
}
