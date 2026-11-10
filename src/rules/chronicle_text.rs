//! The chronicle's texts (`rules::chronicle`): the dry line each entry shows in the «Летопись»
//! tab, the full text of a text window, and the summary's highlight sentences. The templates
//! are English keys whose Russian is in the catalog (`data/lang/ru/game.txt`); the names come
//! from the install (units, items, spells, buildings, armies, events), never from this file.

use super::chronicle::{Entry, EntryKind, FoeRef, Summary, TextRef, MINUTES_PER_DAY};
use super::clock::Clock;
use super::game::Game;
use super::journal::Row;
use super::world::EventId;
use crate::i18n::tr;
use crate::trf;

/// The catalog keys of the templates, so a test can check each has its Russian.
pub const KEYS: &[&str] = &[
    "{text} Answer: {answer}.",
    "Battle with {foe}: {result}. Our losses: {own}, theirs: {theirs}.",
    "Battle: {result}. Our losses: {own}, theirs: {theirs}.",
    "won",
    "lost",
    "Taken: {place}.",
    "Entered: {place}.",
    "Hired: {unit} × {count}.",
    "Dismissed: {unit} × {count}.",
    "Fell outside battle: {unit} × {count}.",
    "Hero's level: {level}.",
    "Spell learnt: {spell}.",
    "Artefact found: {item}.",
    "Artefact bought: {item}.",
    "Quest taken: {quest}.",
    "Quest completed: {quest}.",
    "Noon report: gold {gold}, income {income}, wages {wages}.",
    "Earlier days are not recorded.",
    "A message",
    "unknown",
    "Yes",
    "No",
    "Biggest battle — day {day}: against {foe}, {result} (losses {own} : {theirs})",
    "Biggest battle — day {day}: {result} (losses {own} : {theirs})",
    "Most hired at once — {count} {unit} (day {day})",
    "Richest moment — {gold} (day {day})",
    "Longest walk — {steps} cells (day {day})",
    "Day {day}",
];

/// The tab's rows: every entry of the record, oldest first. A row is the journal's shape: its
/// title is the dry line, its text the full text of a text window, its date the entry's minute
/// (the day headings count from it, see [`super::chronicle::Chronicle::day_of`]) and its
/// chapter the chapter's index.
pub fn rows(game: &Game) -> Vec<Row> {
    let mut out = Vec::new();
    for (ci, chapter) in game.chronicle.chapters.iter().enumerate() {
        for e in &chapter.entries {
            let (dry, full) = body(game, e);
            out.push(Row { title: line_of(e, &dry), text: full.unwrap_or_default(), date: Some(Clock::at_minutes(e.at_minutes)), elapsed: None, chapter: ci as u32 });
        }
    }
    out
}

/// An entry's dry line with its time: «14:00. Битва с …».
fn line_of(e: &Entry, dry: &str) -> String {
    let minute = e.at_minutes % MINUTES_PER_DAY;
    // The time is a plain format (no words to translate), so it is not a catalog line.
    format!("{:02}:{:02}. {dry}", minute / 60, minute % 60)
}

/// The dry text of an entry (without its time) and, for a text window, its full text.
pub fn body(game: &Game, e: &Entry) -> (String, Option<String>) {
    match &e.kind {
        EntryKind::Text { text, answer } => {
            let full = text_full(game, text, answer.is_some());
            let line = full.lines().next().unwrap_or("").to_string();
            let line = if line.is_empty() { tr("A message").to_string() } else { line };
            let dry = match answer {
                Some(yes) => trf!("{text} Answer: {answer}.", text = line, answer = if *yes { tr("Yes") } else { tr("No") }),
                None => line,
            };
            (dry, Some(full).filter(|f| !f.is_empty()))
        }
        EntryKind::Battle { foe, won, own_losses, foe_losses, .. } => {
            let result = if *won { tr("won") } else { tr("lost") };
            let (own, theirs) = (own_losses, foe_losses);
            let dry = match foe.and_then(|f| foe_name(game, f)) {
                Some(foe) => trf!("Battle with {foe}: {result}. Our losses: {own}, theirs: {theirs}.", foe = foe, result = result, own = own, theirs = theirs),
                None => trf!("Battle: {result}. Our losses: {own}, theirs: {theirs}.", result = result, own = own, theirs = theirs),
            };
            (dry, None)
        }
        EntryKind::Captured { location } => (trf!("Taken: {place}.", place = place(game, *location)), None),
        EntryKind::FirstVisit { location } => (trf!("Entered: {place}.", place = place(game, *location)), None),
        EntryKind::Hired { unit, count } => (trf!("Hired: {unit} × {count}.", unit = unit_name(game, unit.0), count = count), None),
        EntryKind::Dismissed { unit, count } => (trf!("Dismissed: {unit} × {count}.", unit = unit_name(game, unit.0), count = count), None),
        EntryKind::Fell { unit, count } => (trf!("Fell outside battle: {unit} × {count}.", unit = unit_name(game, unit.0), count = count), None),
        EntryKind::LevelUp { level } => (trf!("Hero's level: {level}.", level = level), None),
        EntryKind::SpellLearned { spell } => {
            let name = game.content.spell(*spell).map_or_else(|| tr("unknown").to_string(), |s| s.name.clone());
            (trf!("Spell learnt: {spell}.", spell = name), None)
        }
        EntryKind::ArtefactFound { item } => (trf!("Artefact found: {item}.", item = item_name(game, item.0)), None),
        EntryKind::ArtefactBought { item } => (trf!("Artefact bought: {item}.", item = item_name(game, item.0)), None),
        EntryKind::QuestTaken { quest } => (trf!("Quest taken: {quest}.", quest = game.event_title(*quest)), None),
        EntryKind::QuestDone { quest } => (trf!("Quest completed: {quest}.", quest = game.event_title(*quest)), None),
        EntryKind::Noon { gold, income, wages } => {
            (trf!("Noon report: gold {gold}, income {income}, wages {wages}.", gold = gold, income = income, wages = wages), None)
        }
        EntryKind::EarlierDaysNotRecorded => (tr("Earlier days are not recorded.").to_string(), None),
    }
}

/// The summary's four highlight sentences, in the order the page shows them (one for each
/// highlight that exists).
pub fn highlight_lines(game: &Game, s: &Summary) -> Vec<String> {
    let h = &s.highlights;
    let mut out = Vec::new();
    let day = |e: &Entry| game.chronicle.day_of(e.at_minutes);
    if let Some(e) = &h.biggest_battle {
        if let EntryKind::Battle { foe, won, own_losses, foe_losses, .. } = e.kind {
            let result = if won { tr("won") } else { tr("lost") };
            let (own, theirs, day) = (own_losses, foe_losses, day(e));
            out.push(match foe.and_then(|f| foe_name(game, f)) {
                Some(foe) => trf!("Biggest battle — day {day}: against {foe}, {result} (losses {own} : {theirs})", day = day, foe = foe, result = result, own = own, theirs = theirs),
                None => trf!("Biggest battle — day {day}: {result} (losses {own} : {theirs})", day = day, result = result, own = own, theirs = theirs),
            });
        }
    }
    if let Some(e) = &h.most_hired {
        if let EntryKind::Hired { unit, count } = e.kind {
            let (unit, count, day) = (unit_name(game, unit.0), count, day(e));
            out.push(trf!("Most hired at once — {count} {unit} (day {day})", count = count, unit = unit, day = day));
        }
    }
    if let Some(e) = &h.richest {
        if let EntryKind::Noon { gold, .. } = e.kind {
            out.push(trf!("Richest moment — {gold} (day {day})", gold = gold, day = day(e)));
        }
    }
    if let Some((index, steps)) = h.longest_walk {
        let day = game.chronicle.day_of(index * MINUTES_PER_DAY);
        out.push(trf!("Longest walk — {steps} cells (day {day})", steps = steps, day = day));
    }
    out
}

/// The full text of a scenario event as its window shows it: the question when it was asked,
/// else the message (the other when that one is empty). `#HERONAME` filled, the markup dropped.
/// None when the map's script does not hold the event.
pub fn event_text(game: &Game, id: EventId, asked: bool) -> Option<String> {
    let e = game.script()?.event(id)?;
    let (first, second) = if asked { (&e.question, &e.message) } else { (&e.message, &e.question) };
    let raw = if first.trim().is_empty() { second } else { first };
    Some(plain(&raw.replace("#HERONAME", &game.hero_name())))
}

/// A text the record holds: by reference (its event, while the game has its map) or in full.
fn text_full(game: &Game, text: &TextRef, asked: bool) -> String {
    match text {
        TextRef::Event(id) => event_text(game, *id, asked).unwrap_or_default(),
        TextRef::Catalog { key, args } => std::iter::once(tr(key).to_string()).chain(args.iter().cloned()).collect::<Vec<_>>().join(" "),
        TextRef::Text(full) => full.clone(),
    }
}

/// A scenario text as its window shows it: the markup's marks (`* | @ ^`) and the line breaks
/// (CR LF, `#\`) dropped, the paragraphs joined by a line break.
fn plain(raw: &str) -> String {
    raw.replace("#\\", "\n")
        .split(['\r', '\n'])
        .map(|l| l.chars().filter(|c| !matches!(c, '*' | '|' | '@' | '^')).collect::<String>().trim().to_string())
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// The name of a foe: a garrison's building, or an army's name (None when it has none).
fn foe_name(game: &Game, foe: FoeRef) -> Option<String> {
    match foe {
        FoeRef::Garrison(id) => game.world.locations.iter().find(|l| l.id == id).map(|l| l.name.clone()).filter(|n| !n.is_empty()),
        FoeRef::Army(id) => game.world.armies.iter().find(|a| a.id == id).map(|a| a.name.clone()).filter(|n| !n.is_empty()),
    }
}

/// A building's name by its scenario id.
fn place(game: &Game, id: u16) -> String {
    game.world.locations.iter().find(|l| l.id == id).map_or_else(|| tr("unknown").to_string(), |l| l.name.clone())
}

fn unit_name(game: &Game, id: u32) -> String {
    game.content.try_unit(super::content::UnitId(id)).map_or_else(|| tr("unknown").to_string(), |u| u.name.clone())
}

fn item_name(game: &Game, id: u32) -> String {
    game.content.items.iter().find(|i| i.id == id).map_or_else(|| tr("unknown").to_string(), |i| i.name.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every template key has its Russian line in the catalog (a key with no line shows English).
    #[test]
    fn every_template_has_a_russian_line() {
        let ru: Vec<String> = crate::i18n::ru_entries().into_iter().map(|e| e.key).collect();
        for key in KEYS {
            assert!(ru.iter().any(|k| k == key), "no Russian line for {key:?}");
        }
    }

    /// Every kind renders a dry line with no placeholder left, and the summary's highlights name
    /// their entries (the install's names are not needed: the demo content has them).
    #[test]
    fn every_kind_renders_a_dry_line_and_the_highlights_name_their_entries() {
        use super::super::content::{Content, HeroClass, ItemId, UnitId};
        use super::super::game::Game;
        use std::sync::Arc;
        let mut g = Game::new(Arc::new(Content::builtin()), HeroClass::Knight);
        g.chronicle.start("Map", 0);
        let kinds = vec![
            EntryKind::Text { text: TextRef::Text("Hello #HERONAME\r\nSecond *line*".into()), answer: Some(true) },
            EntryKind::Text { text: TextRef::Text(String::new()), answer: None },
            EntryKind::Battle { at: (1, 1), foe: Some(FoeRef::Army(2)), won: true, own_losses: 3, foe_losses: 5 },
            EntryKind::Battle { at: (1, 1), foe: None, won: false, own_losses: 0, foe_losses: 1 },
            EntryKind::Captured { location: 9 },
            EntryKind::FirstVisit { location: 9 },
            EntryKind::Hired { unit: UnitId(4), count: 3 },
            EntryKind::Dismissed { unit: UnitId(4), count: 1 },
            EntryKind::Fell { unit: UnitId(4), count: 2 },
            EntryKind::LevelUp { level: 2 },
            EntryKind::SpellLearned { spell: 3 },
            EntryKind::ArtefactFound { item: ItemId(1) },
            EntryKind::ArtefactBought { item: ItemId(1) },
            EntryKind::QuestTaken { quest: 4 },
            EntryKind::QuestDone { quest: 4 },
            EntryKind::Noon { gold: 900, income: 300, wages: 120 },
            EntryKind::EarlierDaysNotRecorded,
        ];
        for (i, kind) in kinds.iter().enumerate() {
            g.chronicle.record(i as u64 * 60, kind.clone());
        }
        let rows = rows(&g);
        assert_eq!(rows.len(), kinds.len());
        for r in &rows {
            assert!(!r.title.is_empty() && !r.title.contains('{') && !r.title.contains('}'), "{:?}", r.title);
        }
        // The text window: its first line in the list, its whole text in the entry.
        assert_eq!(rows[0].title, "00:00. Hello #HERONAME Answer: Yes.");
        assert_eq!(rows[0].text, "Hello #HERONAME\r\nSecond *line*");
        assert_eq!(rows[1].title, "01:00. A message", "an empty text has no first line");
        assert_eq!(rows[2].text, "", "a battle's line is the whole entry");
        assert_eq!(rows[0].date.map(|d| d.total_minutes()), Some(0.0), "the day headings count from the date");
        // The highlights: the biggest battle (8 units), the most hired (3), the richest noon (900);
        // no walk, so no walk line.
        let s = g.chronicle.summary(0, 10_000, false).expect("a chapter");
        let lines = highlight_lines(&g, &s);
        assert_eq!(lines.len(), 3, "{lines:?}");
        assert_eq!(lines[0], "Biggest battle — day 1: won (losses 3 : 5)", "{lines:?}");
        assert!(lines[1].starts_with("Most hired at once — 3 ") && lines[1].ends_with(" (day 1)"), "{lines:?}");
        assert_eq!(lines[2], "Richest moment — 900 (day 1)", "{lines:?}");
    }

    #[test]
    fn plain_drops_the_markup_and_keeps_the_paragraphs() {
        assert_eq!(plain("^Hello*\r\n\r\n@Two|\r\n"), "Hello\nTwo");
        assert_eq!(plain("One#\\Two"), "One\nTwo");
    }

    #[test]
    fn the_dry_line_carries_its_time_first() {
        let e = Entry { at_minutes: 14 * 60 + 5, kind: EntryKind::EarlierDaysNotRecorded };
        assert_eq!(line_of(&e, "x"), "14:05. x");
        let noon = Entry { at_minutes: 2 * MINUTES_PER_DAY + 9 * 60, kind: EntryKind::EarlierDaysNotRecorded };
        assert_eq!(line_of(&noon, "x"), "09:00. x", "the time of day, not of the whole day count");
    }
}
