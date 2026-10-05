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
use super::widgets::*;
use super::world_view;
use super::Screen;

pub const QUEST_ADDED: &str = n_("Quest added to the journal");
pub const QUEST_COMPLETED: &str = n_("Quest completed");

/// An event's title as shown: without its flag script, escapes filled in.
pub fn event_title(game: &Game, id: EventId) -> String {
    game.event_title(id)
}

/// An event's own picture: `width`, `height` (u16 each) and 16-bit pixels, taken as RGB565
/// *(guess, dtm-format.md §9)*.
fn custom_picture(data: &[u8]) -> Option<Texture2D> {
    let w = u16::from_le_bytes([*data.first()?, *data.get(1)?]);
    let h = u16::from_le_bytes([*data.get(2)?, *data.get(3)?]);
    let px = data.get(4..4 + 2 * w as usize * h as usize)?;
    if w == 0 || h == 0 {
        return None;
    }
    let rgba: Vec<u8> = px
        .chunks_exact(2)
        .flat_map(|p| {
            let v = u16::from_le_bytes([p[0], p[1]]);
            let (r, g, b) = ((v >> 11) & 31, (v >> 5) & 63, v & 31);
            [(r * 255 / 31) as u8, (g * 255 / 63) as u8, (b * 255 / 31) as u8, 255]
        })
        .collect();
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
    let r = &e.results;
    d.picture = match (&e.custom_picture, r.picture) {
        (Some(data), _) if custom_picture(data).is_some() => custom_picture(data).map(Picture::Image),
        (_, 0 | PICTURE_DEFEAT | PICTURE_VICTORY) => None,
        (_, u) => game.content.try_unit(UnitId(u as u32)).map(|_| Picture::Unit(UnitId(u as u32))),
    };
    // "No meeting" events show no resource row (the Community hook 0xc2831f; opcodes use
    // the fields as arguments).
    if asking || r.no_meeting == 1 {
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
    d.items = r.artifacts_add.iter().filter(|&&a| a != 0).map(|&a| ItemId(a as u32)).filter(|&i| game.content.try_item(i).is_some()).collect();
    let known = |u: u8| game.content.try_unit(UnitId(u as u32)).map(|_| UnitId(u as u32));
    d.joined = r.units_add.iter().filter_map(|&u| known(u)).collect();
    d.left = r.units_remove.iter().filter(|&&u| u != 0xFE && u != 0xFF).filter_map(|&u| known(u)).collect();
    d
}

/// Puts an outcome on screen: a dialog, a question, a notice on the dialog just queued (or
/// the message line).
pub fn show(game: &Game, o: &EventOutcome, message: &mut Option<String>, dialogs: &mut VecDeque<Dialog>) {
    match *o {
        EventOutcome::Fired { event, message: true } => dialogs.push_back(event_dialog(game, event, false)),
        EventOutcome::Question(id) => dialogs.push_back(event_dialog(game, id, true)),
        EventOutcome::QuestAdded(id) => notice(tr(QUEST_ADDED), Some(id), message, dialogs),
        EventOutcome::QuestCompleted(_) => notice(tr(QUEST_COMPLETED), None, message, dialogs),
        EventOutcome::LoopGuard => razdor::diag!("scenario events: loop guard reached"),
        EventOutcome::Fired { .. } | EventOutcome::Declined(_) | EventOutcome::Victory(_) | EventOutcome::Defeat(_) => {}
    }
}

/// The notice goes on the last event dialog queued (the event that gave or closed the
/// quest), if it is that one; the message line shows it too.
fn notice(line: &str, event: Option<EventId>, message: &mut Option<String>, dialogs: &mut VecDeque<Dialog>) {
    if let Some(d) = dialogs.back_mut().filter(|d| d.event.is_some() && !d.question && (event.is_none() || d.event == event)) {
        d.add_notice(line);
    }
    *message = Some(line.to_string());
}

/// The journal screen: its tab, the selected line, and how far the list and the text are
/// scrolled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JournalView {
    pub tab: Tab,
    pub selected: usize,
    pub scroll: usize,
    pub text_scroll: usize,
}

impl Default for JournalView {
    fn default() -> Self {
        JournalView { tab: Tab::Active, selected: 0, scroll: 0, text_scroll: 0 }
    }
}

/// What an empty tab says.
fn empty_note(tab: Tab) -> &'static str {
    match tab {
        Tab::Active => tr("No quests under way."),
        Tab::Completed => tr("No quest completed yet."),
        Tab::Rumours => tr("No rumours heard yet."),
        Tab::Messages => tr("No messages yet."),
    }
}

/// The journal (a Razdor extra on top of the original's quest list): tabs for the active
/// quests, those completed, the rumours heard and the story messages, newest first; the
/// list on the left, the selected entry's date and full text on the right. Wheel or arrows
/// scroll; Left / Right change the tab; J, Esc or Back close it.
pub fn journal(game: &Game, assets: &super::assets::Assets, view: &mut JournalView) -> Option<Screen> {
    let bar_pick = world_view::window_backdrop(game, assets, Some(super::game_bar::BarButton::Journal));
    let (sw, sh) = (screen_width(), screen_height());
    let bar = super::chrome::bar_height();
    let (w, h) = (1000.0f32.min(sw - 20.0), 640.0f32.min(sh - bar - 8.0));
    let (x, y) = ((sw - w) / 2.0, ((sh - bar - h) / 2.0).max(4.0));
    super::chrome::window(Rect::new(x, y, w, h), tr("The hero's journal"), super::chrome::Skin::Marble, false);

    // The tabs, with their counts.
    let mut switch = None;
    let tw = ((w - 32.0) / 4.0).min(230.0);
    for (k, tab) in Tab::ALL.into_iter().enumerate() {
        let n = game.journal_rows(tab).len();
        let bx = x + 16.0 + k as f32 * (tw + 4.0);
        if button(bx, y + 36.0, tw, 34.0, &format!("{} ({n})", tab.label()), true) {
            switch = Some(tab);
        }
        if view.tab == tab {
            draw_rectangle_lines(bx - 3.0, y + 33.0, tw + 6.0, 40.0, 2.0, Color::new(1.0, 0.6, 0.2, 1.0));
        }
    }
    let rows = game.journal_rows(view.tab);
    view.selected = view.selected.min(rows.len().saturating_sub(1));

    // The list.
    let (lx, ly, lw) = (x + 16.0, y + 80.0, 360.0f32.min(w * 0.4));
    let lh = h - 146.0;
    super::chrome::parchment(Rect::new(lx, ly, lw, lh), false);
    let ink = Color::new(0.45, 0.28, 0.14, 1.0);
    let rh = 42.0;
    let fits = (((lh - 12.0) / rh).floor() as usize).max(1);
    if mouse_in(lx, ly, lw, lh) {
        let wh = wheel();
        if wh < 0.0 && view.scroll + fits < rows.len() {
            view.scroll += 1;
        } else if wh > 0.0 {
            view.scroll = view.scroll.saturating_sub(1);
        }
    }
    // Keep the selected line in sight.
    if view.selected < view.scroll {
        view.scroll = view.selected;
    } else if view.selected >= view.scroll + fits {
        view.scroll = view.selected + 1 - fits;
    }
    view.scroll = view.scroll.min(rows.len().saturating_sub(fits));
    if rows.is_empty() {
        text_fit(empty_note(view.tab), lx + 16.0, ly + 30.0, lw - 28.0, 18.0, ink);
    }
    for (k, row) in rows.iter().enumerate().skip(view.scroll).take(fits) {
        let ry = ly + 6.0 + (k - view.scroll) as f32 * rh;
        if view.selected == k {
            draw_rectangle(lx + 4.0, ry, lw - 8.0, rh - 2.0, Color::new(0.72, 0.6, 0.4, 1.0));
        }
        let mut title = row.title.clone();
        while measure(&title, 18.0).width > lw - 30.0 && title.chars().count() > 3 {
            title.pop();
        }
        text(&title, lx + 14.0, ry + 18.0, 18.0, ink);
        let date = row.date.map_or_else(|| tr("date unknown").to_string(), |d| d.label());
        text(&date, lx + 14.0, ry + 35.0, 14.0, Color::new(0.4, 0.36, 0.3, 1.0));
        if mouse_in(lx, ry, lw, rh - 2.0) && clicked() && view.selected != k {
            view.selected = k;
            view.text_scroll = 0;
        }
    }
    if rows.len() > fits {
        let line = trf!("{first}–{last} of {total}", first = view.scroll + 1, last = (view.scroll + fits).min(rows.len()), total = rows.len());
        text(&line, lx + 8.0, ly + lh + 18.0, 15.0, DIM);
    }

    // The selected entry: title, date and text.
    let (tx, tw) = (lx + lw + 16.0, w - lw - 48.0);
    super::chrome::text_box(Rect::new(tx, ly, tw, lh));
    let box_ink = Color::new(1.0, 0.86, 0.58, 1.0);
    match rows.get(view.selected) {
        Some(row) => {
            text_centered(&row.title, tx + tw / 2.0, ly + 32.0, 21.0, box_ink);
            let mut sub = row.date.map_or_else(|| tr("Date unknown").to_string(), |d| d.label());
            if view.tab == Tab::Completed {
                sub = format!("{}: {sub}", tr(QUEST_COMPLETED));
            }
            if let Some(m) = row.elapsed {
                sub = trf!("Time since it was received: {t}", t = razdor::rules::clock::duration_label(m as f64));
            }
            text_centered(&sub, tx + tw / 2.0, ly + 56.0, 16.0, super::dialog::MANA);
            let lines: Vec<String> = row
                .text
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .flat_map(|p| {
                    let mut v = wrap(p, tw - 40.0, 18.0);
                    v.push(String::new());
                    v
                })
                .collect();
            let (top, bottom) = (ly + 88.0, ly + lh - 14.0);
            let shown = (((bottom - top) / 22.0).floor() as usize).max(1);
            if mouse_in(tx, ly, tw, lh) {
                let wh = wheel();
                if wh < 0.0 && view.text_scroll + shown < lines.len() {
                    view.text_scroll += 1;
                } else if wh > 0.0 {
                    view.text_scroll = view.text_scroll.saturating_sub(1);
                }
            }
            if key(KeyCode::PageDown) && view.text_scroll + shown < lines.len() {
                view.text_scroll += shown.saturating_sub(1).max(1);
            }
            if key(KeyCode::PageUp) {
                view.text_scroll = view.text_scroll.saturating_sub(shown.saturating_sub(1).max(1));
            }
            view.text_scroll = view.text_scroll.min(lines.len().saturating_sub(shown));
            for (i, line) in lines.iter().skip(view.text_scroll).take(shown).enumerate() {
                text(line, tx + 20.0, top + i as f32 * 22.0, 18.0, box_ink);
            }
            if lines.len() > shown {
                let more = trf!("{first}–{last} of {total} lines (wheel, PgUp / PgDn)", first = view.text_scroll + 1, last = (view.text_scroll + shown).min(lines.len()), total = lines.len());
                text(&more, tx + 8.0, ly + lh + 18.0, 15.0, DIM);
            }
        }
        None => text_centered(tr("What the hero learns is written here."), tx + tw / 2.0, ly + lh / 2.0, 19.0, box_ink),
    }

    let back = button(x + w / 2.0 - 70.0, y + h - 50.0, 140.0, 38.0, tr("Back"), true);
    if back || key(KeyCode::Escape) || key(KeyCode::J) {
        return Some(Screen::WorldMap);
    }
    let at = Tab::ALL.iter().position(|&t| t == view.tab).unwrap_or(0);
    if key(KeyCode::Right) {
        switch = Some(Tab::ALL[(at + 1) % Tab::ALL.len()]);
    }
    if key(KeyCode::Left) {
        switch = Some(Tab::ALL[(at + Tab::ALL.len() - 1) % Tab::ALL.len()]);
    }
    if let Some(tab) = switch.filter(|&t| t != view.tab) {
        *view = JournalView { tab, ..JournalView::default() };
        return None;
    }
    if key(KeyCode::Down) && view.selected + 1 < rows.len() {
        view.selected += 1;
        view.text_scroll = 0;
    }
    if key(KeyCode::Up) && view.selected > 0 {
        view.selected -= 1;
        view.text_scroll = 0;
    }
    bar_pick
}
