//! What the scenario's event engine did, on screen: story and quest dialogs (video notes
//! §5), the "added to the journal" / "completed" notices, and the quest journal.
//!
//! Every text comes from the loaded scenario at runtime (with `#HERONAME` filled in).

use std::collections::VecDeque;

use macroquad::prelude::*;

use razdor::i18n::{n_, tr};
use razdor::rules::content::{ItemId, UnitId};
use razdor::trf;
use razdor::rules::events::{ArmyId, EventId, EventOutcome, PICTURE_DEFEAT, PICTURE_LEADER, PICTURE_VICTORY};
use razdor::rules::game::Game;
use razdor::rules::journal::Tab;

use super::items_view::DiaryTab;

use super::dialog::{Dialog, Picture, Resource};

pub const QUEST_ADDED: &str = n_("Quest added to the journal");
pub const QUEST_COMPLETED: &str = n_("Quest completed");

/// An event's title as shown: without its flag script, escapes filled in.
pub fn event_title(game: &Game, id: EventId) -> String {
    game.event_title(id)
}

/// An event's own picture, decoded as the original draws it (ARGB4444, 0x4d1693;
/// `dtm::event_picture_rgba`).
fn custom_picture(data: &[u8]) -> Option<Texture2D> {
    let (w, h, rgba) = razdor::dt::dtm::event_picture_rgba(data)?;
    if w == 0 || h == 0 {
        return None;
    }
    let tex = Texture2D::from_rgba8(w, h, &rgba);
    tex.set_filter(FilterMode::Nearest);
    Some(tex)
}

/// The window of event `id`: its question (`asking`), or its message with what it gave.
pub fn event_dialog(game: &Game, id: EventId, asking: bool) -> Dialog {
    let mut d = Dialog::new(event_title(game, id));
    d.event = Some(id);
    d.question = asking;
    let Some(e) = game.script().and_then(|s| s.event(id)) else { return d };
    let body = if asking && !e.question.trim().is_empty() { &e.question } else { &e.message };
    // The original fills in `#HERONAME` (0x471f0c) and reads the result as markup (0x48e438:
    // lines at CR LF or `#\`, the marks `*` `|` `@` `^`); the CR is kept for that.
    d.marked = Some(body.replace("#HERONAME", &game.hero_name()));
    // The defeat look (the event window's mode 2, 0x4a8ae8): a title holding `#DEFEAT`, or
    // the map's defeat event. It changes the frame and the music, it does not end the game.
    d.defeat = e.title.contains("#DEFEAT") || game.script().is_some_and(|s| s.is_defeat_event(id));
    let r = &e.results;
    d.picture = match (&e.custom_picture, r.picture) {
        (Some(data), _) if custom_picture(data).is_some() => custom_picture(data).map(Picture::Image),
        // 200 and 201: the defeat and victory pictures, `Stnd-1` and `Stnd-2` (the table
        // 0x674638 + 4·v, loaded at 0x4dc871 / 0x4dc880; 0x4aa105).
        (_, PICTURE_DEFEAT) => super::chrome::win_ugs("Stnd-1").map(Picture::Still),
        (_, PICTURE_VICTORY) => super::chrome::win_ugs("Stnd-2").map(Picture::Still),
        // 202: the leader of the army of the last contact (0x68dc90); none when it has none.
        (_, PICTURE_LEADER) => game.last_met_leader().map(Picture::Unit),
        (_, 0) => None,
        (_, u) => game.content.try_unit(UnitId(u as u32)).map(|_| Picture::Unit(UnitId(u as u32))),
    };
    if asking {
        return d;
    }
    // The spells it teaches are shown even with no meeting (c28604 skips only the learning).
    d.spells = r.spells_learned.iter().filter(|&&s| s != 0).map(|&s| s as u32).filter(|&s| game.content.spells.iter().any(|x| x.id == s)).collect();
    let known = |u: u8| game.content.try_unit(UnitId(u as u32)).map(|_| UnitId(u as u32));
    d.joined = r.units_add.iter().filter_map(|&u| known(u)).collect();
    // The departed row is not built here: it lists the units the event really removed, which
    // the engine reports as `EventOutcome::UnitsLost` right after `Fired` (`show`).
    // The items gained are left out only for opcode 6, whose fields are its arguments
    // (c278e8).
    if !(r.no_meeting == 1 && r.patrol_delta == 6) {
        d.items = r.artifacts_add.iter().filter(|&&a| a != 0).map(|&a| ItemId(a as u32)).filter(|&i| game.content.try_item(i).is_some()).collect();
    }
    // "No meeting" events show no resource row (the Community hook 0xc2831f; opcodes use
    // the fields as arguments).
    if r.no_meeting == 1 {
        return d;
    }
    let signed = |v: i16| if v < 0 { format!("- {}", -(v as i32)) } else { format!("+ {v}") };
    if r.gold != 0 {
        d.resources.push((Resource::Gold, trf!("Gold {n}", n = signed(r.gold))));
    }
    if r.mana != 0 {
        d.resources.push((Resource::Mana, trf!("Mana {n}", n = signed(r.mana))));
    }
    if r.experience != 0 {
        d.resources.push((Resource::Experience, trf!("Experience {n}", n = signed(r.experience))));
    }
    d
}

/// The caption under the picture of an event met on army `army` (Razdor's option
/// `Game::leader_caption`): the army's leader, as its tooltip names him. None when the option is
/// off, the army is gone, or it has no leader.
pub fn leader_caption(game: &Game, army: ArmyId) -> Option<String> {
    if !game.leader_caption {
        return None;
    }
    let a = game.world.armies.iter().chain(&game.world.inactive).find(|a| a.id == army)?;
    leader_text(&a.leader_name)
}

/// A leader's name as the caption shows it: spaces collapsed and the ends trimmed; None when
/// nothing is left.
pub fn leader_text(name: &str) -> Option<String> {
    let s = razdor::dt::dtm::collapse_spaces(name);
    let s = s.trim();
    (!s.is_empty()).then(|| s.to_string())
}

/// Puts an outcome on screen: a dialog, a question, a quest line on the dialog just queued.
pub fn show(game: &Game, o: &EventOutcome, dialogs: &mut VecDeque<Dialog>) {
    match *o {
        EventOutcome::Fired { event, message: true } => dialogs.push_back(event_dialog(game, event, false)),
        EventOutcome::Question(id) => dialogs.push_back(event_dialog(game, id, true)),
        // The row goes on the event's own window, just queued (0x4a8fe5 builds it with the
        // window); a silent event has no window to show it in.
        EventOutcome::ItemsLost { event, ref items } => {
            if let Some(d) = dialogs.back_mut().filter(|d| d.event == Some(event)) {
                d.lost = items.iter().map(|&a| ItemId(a as u32)).collect();
            }
        }
        // The departed row: the unit types really removed (0x4a9711), one icon each. A question
        // shows none before its answer (`event_dialog` returns before the rows).
        EventOutcome::UnitsLost { event, ref units } => {
            if let Some(d) = dialogs.back_mut().filter(|d| d.event == Some(event)) {
                d.left = units.iter().filter(|&&u| game.content.try_unit(UnitId(u)).is_some()).map(|&u| UnitId(u)).collect();
            }
        }
        // The army's leader under the picture (`leader_caption`), on the window of the event met.
        EventOutcome::MeetingArmy { event, army } => {
            if let Some(d) = dialogs.back_mut().filter(|d| d.event == Some(event) && d.picture.is_some()) {
                d.caption = leader_caption(game, army);
            }
        }
        EventOutcome::QuestAdded(id) => notice(&quest_line("AddQuest", QUEST_ADDED), id, dialogs),
        EventOutcome::QuestCompleted { by, .. } => notice(&quest_line("DelQuest", QUEST_COMPLETED), by, dialogs),
        EventOutcome::LoopGuard => razdor::diag!("scenario events: loop guard reached"),
        EventOutcome::ChainCut(id) => razdor::diag!("scenario events: chain at event {id} cut (depth limit)"),
        EventOutcome::Fired { .. } | EventOutcome::Declined(_) | EventOutcome::Victory(_) | EventOutcome::Defeat(_) => {}
    }
}

/// The event window's quest line: `[Event] AddQuest` («Новая запись добавлена в дневник
/// героя!») or `DelQuest` from the install, as 0x4a8ae8 sets it (0x4a8de4, 0x4a8e12); Razdor's
/// own wording in English or without the install's text.
fn quest_line(key: &str, ours: &'static str) -> String {
    let t = (razdor::i18n::lang() == razdor::i18n::Lang::Ru).then(|| super::chrome::ui_text("Event", key)).flatten();
    t.unwrap_or_else(|| tr(ours).to_string())
}

/// The quest line goes under the event's window, as the original's (0x4a8ae8, 0x4a8e12): on the
/// dialog just queued, if it is the window of `event` (the one that gave or closed the quest)
/// and not a question. Otherwise it is dropped; there is no message-line fallback, the
/// original has none (a silent event has no window to carry it).
fn notice(line: &str, event: EventId, dialogs: &mut VecDeque<Dialog>) {
    if let Some(d) = dialogs.back_mut().filter(|d| d.event == Some(event) && !d.question) {
        let quest = [quest_line("AddQuest", QUEST_ADDED), quest_line("DelQuest", QUEST_COMPLETED)];
        d.notice = quest_notice(d.notice.take(), line, &quest);
    }
}

/// The window's notice after quest line `line` (`quest`: the added and the completed line).
/// The original keeps one quest line in one label (0x67483f): an event that gives a quest and
/// closes one shows only the completed line, written after the added one (0x4a8de4, then
/// 0x4a8e12), whatever order Razdor reports them in. Other notices are kept beside it.
fn quest_notice(current: Option<String>, line: &str, quest: &[String; 2]) -> Option<String> {
    match current {
        Some(n) if n == quest[1] && line == quest[0] => Some(n),
        Some(n) if quest.contains(&n) => Some(line.to_string()),
        Some(n) => Some(format!("{n}   {line}")),
        None => Some(line.to_string()),
    }
}

/// What an empty tab says.
pub(super) fn empty_note(tab: DiaryTab) -> &'static str {
    match tab {
        DiaryTab::Journal(Tab::Active) => tr("No quests under way."),
        DiaryTab::Journal(Tab::Completed) => tr("No quest completed yet."),
        DiaryTab::Journal(Tab::Rumours) => tr("No rumours heard yet."),
        DiaryTab::Journal(Tab::Messages) => tr("No messages yet."),
        DiaryTab::Chronicle => tr("Nothing written down yet."),
    }
}

/// A line of the journal's list: a chapter's heading, an entry (by index in the rows), or, in
/// the chronicle, a day's heading (the game day's number).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Line {
    Heading(u32),
    Row(usize),
    Day(u64),
}

/// The chronicle's lines: each chapter under its heading and each day under its «День N»
/// heading (a day that starts a chapter's run gets both), one line per entry. `day` gives an
/// entry's day number.
pub(super) fn chronicle_lines(rows: &[razdor::rules::journal::Row], day: impl Fn(&razdor::rules::journal::Row) -> u64) -> Vec<Line> {
    let mut lines = Vec::with_capacity(rows.len() * 2 + 4);
    for (k, row) in rows.iter().enumerate() {
        let (new_chapter, new_day) = match k.checked_sub(1).map(|p| &rows[p]) {
            None => (true, true),
            Some(prev) => (prev.chapter != row.chapter, day(prev) != day(row)),
        };
        if new_chapter {
            lines.push(Line::Heading(row.chapter));
        }
        if new_day {
            lines.push(Line::Day(day(row)));
        }
        lines.push(Line::Row(k));
    }
    lines
}

/// One item of the chronicle's list as drawn: a chapter's heading, a day's heading, one line of an
/// entry's title wrapped to the list's width (`more`: another line of the same entry follows it), or
/// a gap of half a line between two entries and around a day's heading.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Visual {
    Heading(u32),
    Day(u64),
    Text { line: String, more: bool },
    Gap,
}

/// The height of a gap, in steps (a step is one line of the list).
pub(super) const GAP_UNITS: f32 = 0.5;
/// The height of a line that a wrapped entry runs on from (+15 % of a step); the last line of an
/// entry and the headings take one step.
pub(super) const WRAP_UNITS: f32 = 1.15;

impl Visual {
    /// The height of this item in steps; the list's scroll and its fit are counted in these.
    pub(super) fn units(&self) -> f32 {
        match self {
            Visual::Gap => GAP_UNITS,
            Visual::Text { more: true, .. } => WRAP_UNITS,
            _ => 1.0,
        }
    }
}

/// Adds a gap unless the list is empty or already ends in one.
fn pad(out: &mut Vec<Visual>) {
    if out.last().is_some_and(|v| *v != Visual::Gap) {
        out.push(Visual::Gap);
    }
}

/// The chronicle's list as drawn (`chronicle_lines`, each entry wrapped): every heading and day
/// is one item, and an entry takes as many lines as `wrap` cuts its title into (at least one, so
/// an empty title keeps its place). A gap goes before each entry, heading and day, and after each
/// day; the list scrolls and fits by the items' `units`.
pub(super) fn chronicle_visual(lines: &[Line], rows: &[razdor::rules::journal::Row], wrap: impl Fn(&str) -> Vec<String>) -> Vec<Visual> {
    let mut out = Vec::with_capacity(lines.len() * 2);
    for line in lines {
        match *line {
            Line::Heading(chapter) => {
                pad(&mut out);
                out.push(Visual::Heading(chapter));
            }
            Line::Day(day) => {
                pad(&mut out);
                out.push(Visual::Day(day));
                out.push(Visual::Gap);
            }
            Line::Row(k) => {
                pad(&mut out);
                let mut cut = wrap(&rows[k].title);
                if cut.is_empty() {
                    cut.push(String::new());
                }
                let n = cut.len();
                out.extend(cut.into_iter().enumerate().map(|(i, line)| Visual::Text { line, more: i + 1 < n }));
            }
        }
    }
    out
}

/// The first item the list shows when it is scrolled to its end: the fewest items from the foot
/// whose heights fit in `avail` steps (all of them when they fit).
pub(super) fn scroll_end(visual: &[Visual], avail: f32) -> usize {
    let mut sum = 0.0;
    let mut start = visual.len();
    for (i, v) in visual.iter().enumerate().rev() {
        if sum + v.units() > avail + 1e-3 {
            break;
        }
        sum += v.units();
        start = i;
    }
    start
}

/// How many items from `from` fit in `avail` steps.
pub(super) fn visible_from(visual: &[Visual], from: usize, avail: f32) -> usize {
    let mut sum = 0.0;
    let mut n = 0;
    for v in visual.iter().skip(from) {
        if sum + v.units() > avail + 1e-3 {
            break;
        }
        sum += v.units();
        n += 1;
    }
    n
}

/// The list's lines: the rows, each run of one chapter under its heading when the rows come
/// from more than one.
pub(super) fn journal_lines(rows: &[razdor::rules::journal::Row]) -> Vec<Line> {
    let several = rows.windows(2).any(|w| w[0].chapter != w[1].chapter);
    let mut lines = Vec::with_capacity(rows.len() + 4);
    for (k, row) in rows.iter().enumerate() {
        if several && (k == 0 || rows[k - 1].chapter != row.chapter) {
            lines.push(Line::Heading(row.chapter));
        }
        lines.push(Line::Row(k));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use razdor::rules::journal::Row;

    /// The caption is the leader's name as the tooltip writes it: spaces collapsed, the ends
    /// trimmed; no name, no caption.
    #[test]
    fn the_leader_caption_is_trimmed_and_collapsed_and_empty_is_none() {
        assert_eq!(leader_text("  Бенедикт   фон  Вайтлог "), Some("Бенедикт фон Вайтлог".to_string()));
        assert_eq!(leader_text("Бенедикт"), Some("Бенедикт".to_string()));
        assert_eq!(leader_text(" \t "), None);
        assert_eq!(leader_text(""), None);
    }

    fn row(chapter: u32) -> Row {
        Row { title: String::new(), text: String::new(), date: None, elapsed: None, chapter }
    }

    #[test]
    fn one_quest_line_the_completed_one_wins() {
        let q = ["added".to_string(), "done".to_string()];
        assert_eq!(quest_notice(None, "added", &q).as_deref(), Some("added"));
        assert_eq!(quest_notice(Some("added".into()), "done", &q).as_deref(), Some("done"));
        assert_eq!(quest_notice(Some("done".into()), "added", &q).as_deref(), Some("done"), "whatever the order");
        assert_eq!(quest_notice(Some("other".into()), "done", &q).as_deref(), Some("other   done"));
    }

    /// The quest line goes on the window of the event that gave or closed the quest, and on no
    /// other: a closing event with no window of its own leaves the dialogs alone (no toast).
    #[test]
    fn a_quest_line_only_goes_on_the_window_of_the_event_that_gave_or_closed_the_quest() {
        let game = Game::new(std::sync::Arc::new(razdor::rules::content::Content::builtin()), razdor::rules::content::HeroClass::Knight);
        let mut dialogs = VecDeque::from([Dialog { event: Some(5), ..Dialog::new("window") }]);
        show(&game, &EventOutcome::QuestCompleted { quest: 1, by: 7 }, &mut dialogs);
        assert_eq!(dialogs[0].notice, None, "another event's closing leaves this window alone");
        show(&game, &EventOutcome::QuestCompleted { quest: 1, by: 5 }, &mut dialogs);
        assert!(dialogs[0].notice.is_some(), "the closing event's own window carries the line");
    }

    /// The departed row is what the engine removed (0x4a9711): one icon per unit type it reports,
    /// none for a window still asking its question (the rows come with the answer).
    #[test]
    fn the_departed_row_shows_the_units_really_removed() {
        let game = Game::new(std::sync::Arc::new(razdor::rules::content::Content::builtin()), razdor::rules::content::HeroClass::Knight);
        let unit = game.content.units[0].id;
        let other = game.content.units[1].id;
        let window = || VecDeque::from([Dialog { event: Some(75), ..Dialog::new("window") }]);
        // Two removal slots for one unit in the army: one icon.
        let mut dialogs = window();
        show(&game, &EventOutcome::UnitsLost { event: 75, units: vec![unit] }, &mut dialogs);
        assert_eq!(dialogs[0].left, vec![UnitId(unit)]);
        // Two units taken: two icons, in removal order.
        let mut dialogs = window();
        show(&game, &EventOutcome::UnitsLost { event: 75, units: vec![unit, other] }, &mut dialogs);
        assert_eq!(dialogs[0].left, vec![UnitId(unit), UnitId(other)]);
        // A type the content does not know gives no icon.
        let mut dialogs = window();
        show(&game, &EventOutcome::UnitsLost { event: 75, units: vec![9999] }, &mut dialogs);
        assert!(dialogs[0].left.is_empty());
        // Not for another event's window.
        let mut dialogs = window();
        show(&game, &EventOutcome::UnitsLost { event: 76, units: vec![unit] }, &mut dialogs);
        assert!(dialogs[0].left.is_empty());
        // A question window builds no departed row before its answer.
        let asking = event_dialog(&game, 75, true);
        assert!(asking.left.is_empty());
    }

    #[test]
    fn the_journal_list_heads_each_chapter_when_there_are_several() {
        assert_eq!(journal_lines(&[row(0), row(0)]), [Line::Row(0), Line::Row(1)], "one map: no heading");
        assert_eq!(
            journal_lines(&[row(2), row(2), row(0)]),
            [Line::Heading(2), Line::Row(0), Line::Row(1), Line::Heading(0), Line::Row(2)]
        );
    }

    /// The chronicle's list: a wrapped entry's lines run 15 % apart, a gap of half a step goes
    /// between entries and around each day, and the heights add up to the list's height in steps.
    #[test]
    fn the_chronicle_list_counts_its_wrapped_lines_and_gaps() {
        let titled = |title: &str| Row { title: title.into(), text: String::new(), date: None, elapsed: None, chapter: 0 };
        let rows = [titled("abcdefghijkl"), titled(""), titled("xy")];
        // A stand-in wrap: the title cut into pieces of at most five characters.
        let wrap = |t: &str| t.chars().collect::<Vec<_>>().chunks(5).map(|c| c.iter().collect::<String>()).collect::<Vec<_>>();
        let lines = [Line::Heading(0), Line::Day(3), Line::Row(0), Line::Row(1), Line::Row(2)];
        let visual = chronicle_visual(&lines, &rows, wrap);
        let text = |line: &str, more: bool| Visual::Text { line: line.into(), more };
        assert_eq!(
            visual,
            [
                Visual::Heading(0),
                Visual::Gap,
                Visual::Day(3),
                Visual::Gap,
                text("abcde", true),
                text("fghij", true),
                text("kl", false),
                Visual::Gap,
                text("", false),
                Visual::Gap,
                text("xy", false),
            ]
        );
        let height: f32 = visual.iter().map(Visual::units).sum();
        assert!((height - 9.3).abs() < 1e-4, "1 + .5 + 1 + .5 + 1.15 + 1.15 + 1 + .5 + 1 + .5 + 1 steps, got {height}");
        assert!(chronicle_visual(&[], &rows, wrap).is_empty());
    }

    /// Scrolled to its end, the list shows the fewest items from the foot that fit; from the top,
    /// as many as fit.
    #[test]
    fn the_chronicle_list_scrolls_and_fits_by_its_heights() {
        let text = |line: &str, more: bool| Visual::Text { line: line.into(), more };
        let visual = [Visual::Heading(0), Visual::Gap, Visual::Day(3), Visual::Gap, text("abcde", true), text("fghij", true), text("kl", false), Visual::Gap, text("", false), Visual::Gap, text("xy", false)];
        // From the foot: xy 1, gap .5, empty 1, gap .5 make 3; the kl line (1) would make 4.
        assert_eq!(scroll_end(&visual, 3.0), 7);
        assert_eq!(scroll_end(&visual, 100.0), 0, "all of them fit");
        assert_eq!(scroll_end(&[], 3.0), 0);
        // From the top: heading 1 and gap .5 make 1.5; the day would make 2.5, past 2.0.
        assert_eq!(visible_from(&visual, 0, 2.0), 2);
        assert_eq!(visible_from(&visual, 7, 3.0), 4);
    }
}

