//! What the scenario's event engine did, on screen: story and quest dialogs (video notes
//! §5), the "added to the journal" / "completed" notices, and the quest journal.
//!
//! Every text comes from the loaded scenario at runtime (with `#HERONAME` filled in).

use std::collections::VecDeque;

use macroquad::prelude::*;

use razdor::i18n::{n_, tr};
use razdor::rules::content::{ItemId, UnitId};
use razdor::trf;
use razdor::rules::events::{EventId, EventOutcome, PICTURE_DEFEAT, PICTURE_VICTORY};
use razdor::rules::game::Game;
use razdor::rules::journal::Tab;

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
    d.left = r.units_remove.iter().filter(|&&u| u != 0xFE && u != 0xFF).filter_map(|&u| known(u)).collect();
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

/// Puts an outcome on screen: a dialog, a question, a notice on the dialog just queued (or
/// the message line).
pub fn show(game: &Game, o: &EventOutcome, message: &mut Option<String>, dialogs: &mut VecDeque<Dialog>) {
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
        EventOutcome::QuestAdded(id) => notice(&quest_line("AddQuest", QUEST_ADDED), Some(id), message, dialogs),
        EventOutcome::QuestCompleted(_) => notice(&quest_line("DelQuest", QUEST_COMPLETED), None, message, dialogs),
        EventOutcome::LoopGuard => razdor::diag!("scenario events: loop guard reached"),
        EventOutcome::Fired { .. } | EventOutcome::Declined(_) | EventOutcome::Victory(_) | EventOutcome::Defeat(_) => {}
    }
}

/// The notice goes on the last event dialog queued (the event that gave or closed the
/// quest), if it is that one; the message line shows it too.
/// The event window's quest line: `[Event] AddQuest` («Новая запись добавлена в дневник
/// героя!») or `DelQuest` from the install, as 0x4a8ae8 sets it (0x4a8de4, 0x4a8e12); Razdor's
/// own wording in English or without the install's text.
fn quest_line(key: &str, ours: &'static str) -> String {
    let t = (razdor::i18n::lang() == razdor::i18n::Lang::Ru).then(|| super::chrome::ui_text("Event", key)).flatten();
    t.unwrap_or_else(|| tr(ours).to_string())
}

/// The quest line goes under the event's window, as the original's (0x4a8ae8); the map's
/// own toast only when no window shows it (the original has no toast).
fn notice(line: &str, event: Option<EventId>, message: &mut Option<String>, dialogs: &mut VecDeque<Dialog>) {
    if let Some(d) = dialogs.back_mut().filter(|d| d.event.is_some() && !d.question && (event.is_none() || d.event == event)) {
        let quest = [quest_line("AddQuest", QUEST_ADDED), quest_line("DelQuest", QUEST_COMPLETED)];
        d.notice = quest_notice(d.notice.take(), line, &quest);
        return;
    }
    *message = Some(line.to_string());
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
pub(super) fn empty_note(tab: Tab) -> &'static str {
    match tab {
        Tab::Active => tr("No quests under way."),
        Tab::Completed => tr("No quest completed yet."),
        Tab::Rumours => tr("No rumours heard yet."),
        Tab::Messages => tr("No messages yet."),
    }
}

/// A line of the journal's list: a chapter's heading or an entry (by index in the rows).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Line {
    Heading(u32),
    Row(usize),
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

    #[test]
    fn the_journal_list_heads_each_chapter_when_there_are_several() {
        assert_eq!(journal_lines(&[row(0), row(0)]), [Line::Row(0), Line::Row(1)], "one map: no heading");
        assert_eq!(
            journal_lines(&[row(2), row(2), row(0)]),
            [Line::Heading(2), Line::Row(0), Line::Row(1), Line::Heading(0), Line::Row(2)]
        );
    }
}
