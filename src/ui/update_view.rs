//! The windows of `razdor::update`: the offer of a newer release ("Ask"), and once the
//! player chose to update, how it went. They open only at a calm moment (the title screen,
//! the settings, the world map with nothing else on it), never in a battle or over a
//! dialog. A check that fails or finds nothing shows nothing, unless the player asked for it
//! ("Check now" in the advanced settings, which shows [`status_label`]).

use std::cell::Cell;

use macroquad::prelude::*;

use razdor::i18n::tr;
use razdor::trf;
use razdor::update::{self, Mode, Note, Release, Status};

use super::audio::{cue, Cue};
use super::chrome;
use super::widgets::*;

thread_local! {
    /// The check at start was started.
    static STARTED: Cell<bool> = const { Cell::new(false) };
    /// The player asked for the check ("Check now"): its release is offered in any mode.
    static BY_HAND: Cell<bool> = const { Cell::new(false) };
    /// "Later": the offer stays closed until the next start (or the next "Check now").
    static LATER: Cell<bool> = const { Cell::new(false) };
    /// The player chose "Update": the window tells how it went.
    static ASKED: Cell<bool> = const { Cell::new(false) };
    /// That window was closed.
    static TOLD: Cell<bool> = const { Cell::new(false) };
    /// How far the list of changes is scrolled, in screen pixels.
    static SCROLL: Cell<f32> = const { Cell::new(0.0) };
}

/// Once per frame: the check at the first frame (the window is open by then, and the check
/// runs on its own thread), and "Always" puts a release found in place.
pub fn tick(mode: Mode) {
    if !STARTED.with(|s| s.replace(true)) && mode != Mode::Off && update::enabled() {
        update::check();
    }
    if let (Mode::Always, Status::Found(r)) = (mode, update::status()) {
        update::install(r);
    }
}

/// "Check now": a check whose release is offered whatever the mode.
pub fn check_by_hand() {
    BY_HAND.with(|b| b.set(true));
    LATER.with(|l| l.set(false));
    update::check();
}

/// The update window wants to be on screen, at a calm moment.
pub fn wanted(mode: Mode, calm: bool) -> bool {
    calm && match update::status() {
        Status::Found(_) => mode != Mode::Always && (mode == Mode::Ask || BY_HAND.with(|b| b.get())) && !LATER.with(|l| l.get()),
        Status::Installed(_) | Status::InstallFailed(..) => ASKED.with(|a| a.get()) && !TOLD.with(|t| t.get()),
        _ => false,
    }
}

/// The snapshot scene `update`: a made-up newer release on offer.
pub fn stage_offer() {
    STARTED.with(|s| s.set(true));
    update::pretend(Status::Found(Release {
        version: update::Version(9, 9, 9),
        page: "https://github.com/indicozy/razdor/releases".into(),
        program_url: String::new(),
        sums_url: String::new(),
        notes: vec![
            Note::Release(update::Version(9, 9, 9)),
            Note::Section("Добавлено".into()),
            Note::Item("Обновления: Раздор сам находит новую версию на GitHub, предлагает её в окне и ставит на место старой. В настройках («Ещё…»): спрашивать, всегда или выключены.".into()),
            Note::Item("Окно обновления показывает, что изменилось.".into()),
            Note::Section("Исправлено".into()),
            Note::Item("Павший в бою отряд сохраняет свои предметы.".into()),
        ],
    }));
}

/// The state for the advanced settings, next to "Check now".
pub fn status_label() -> String {
    match update::status() {
        Status::Idle => String::new(),
        Status::Checking => tr("Checking…").into(),
        Status::UpToDate => trf!("Up to date ({v})", v = update::current()),
        Status::CheckFailed(_) => tr("Could not check").into(),
        Status::Found(r) => trf!("Razdor {v} is out", v = r.version),
        Status::Downloading(r) => trf!("Downloading {v}…", v = r.version),
        Status::Installed(r) => trf!("{v} starts next time", v = r.version),
        Status::InstallFailed(..) => tr("Could not update").into(),
    }
}

/// The title screen's line when an update is in place (an "Always" update says nothing
/// else).
pub fn title_line() {
    if let Status::Installed(r) = update::status() {
        let k = chrome::k();
        let line = trf!("Razdor {v} is installed and starts next time", v = r.version);
        text_centered(&line, screen_width() / 2.0, screen_height() - 44.0 * k, 16.0 * k, chrome::GOLD);
    }
}

/// A row of buttons from the right edge of `inner`; the index of the one clicked.
fn buttons(inner: Rect, labels: &[&str]) -> Option<usize> {
    let k = chrome::k();
    let (w, h, gap) = (150.0 * k, 28.0 * k, 12.0 * k);
    let mut x = inner.x + inner.w - 24.0 * k - labels.len() as f32 * (w + gap) + gap;
    let y = inner.y + inner.h - 44.0 * k;
    let mut picked = None;
    for (i, label) in labels.iter().enumerate() {
        let r = Rect::new(x, y, w, h);
        let over = r.contains(pointer().into()) && !input_blocked();
        chrome::marble_button(r, label, true, over);
        if over && clicked() {
            cue(Cue::Button);
            picked = Some(i);
        }
        x += w + gap;
    }
    picked
}

/// The list of changes in `r`, scrolled by the wheel and the arrow keys.
fn notes(r: Rect, notes: &[Note]) {
    let k = chrome::k();
    draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.0, 0.0, 0.0, 0.35));
    chrome::silver_frame(r, 1.5);
    let size = 13.0 * k;
    let line = 18.0 * k;
    let (pad, bullet) = (12.0 * k, 16.0 * k);
    let room = r.w - 2.0 * pad;
    // Each row: its text, where it starts, its colour, its size, the space above it.
    let mut rows: Vec<(String, f32, Color, f32, f32)> = Vec::new();
    for n in notes {
        match n {
            Note::Release(v) => rows.push((trf!("Razdor {v}", v), 0.0, chrome::GOLD, 16.0 * k, if rows.is_empty() { 0.0 } else { 10.0 * k })),
            Note::Section(s) => rows.push((s.clone(), 0.0, chrome::CREAM, 14.0 * k, 4.0 * k)),
            Note::Item(s) => {
                for (i, l) in wrap(s, room - bullet, size).into_iter().enumerate() {
                    let l = if i == 0 { format!("•  {l}") } else { l };
                    rows.push((l, if i == 0 { 0.0 } else { bullet }, WHITE, size, if i == 0 { 2.0 * k } else { 0.0 }));
                }
            }
            Note::Text(s) => rows.extend(wrap(s, room, size).into_iter().map(|l| (l, 0.0, WHITE, size, 0.0))),
        }
    }
    let total: f32 = rows.iter().map(|(_, _, _, s, gap)| gap + s.max(line)).sum::<f32>() + 2.0 * pad;
    let most = (total - r.h).max(0.0);
    let mut scroll = SCROLL.with(|s| s.get());
    if r.contains(pointer().into()) {
        scroll -= wheel() * 3.0 * line;
    }
    if key(KeyCode::Down) {
        scroll += 3.0 * line;
    }
    if key(KeyCode::Up) {
        scroll -= 3.0 * line;
    }
    let scroll = scroll.clamp(0.0, most);
    SCROLL.with(|s| s.set(scroll));
    let _clip = Clip::new(r);
    let mut y = r.y + pad - scroll;
    for (text, indent, color, size, gap) in rows {
        y += gap + size.max(line);
        if y > r.y && y - size < r.y + r.h {
            chrome::shadow_text(&text, r.x + pad + indent, y - 4.0 * k, size, color);
        }
    }
    // More below or above: a hint at the edge.
    if scroll < most {
        text_centered("▼", r.x + r.w - 14.0 * k, r.y + r.h - 6.0 * k, 12.0 * k, chrome::GOLD);
    }
    if scroll > 0.0 {
        text_centered("▲", r.x + r.w - 14.0 * k, r.y + 16.0 * k, 12.0 * k, chrome::GOLD);
    }
}

/// Draws the update window over the screen and takes its clicks and keys. `on_title`: the
/// title screen or its settings, where "Restart now" loses no game. True when the player
/// asked to restart into the new version (the caller quits; `update::restart` starts it).
/// "Always update" sets the settings' `mode` to [`Mode::Always`] and updates now.
pub fn frame(on_title: bool, mode: &mut Mode) -> bool {
    let k = chrome::k();
    let status = update::status();
    let offer = matches!(&status, Status::Found(r) if !r.notes.is_empty());
    let (w, h) = if offer { (680.0 * k, (520.0 * k).min(screen_height() - 16.0 * k)) } else { (600.0 * k, 190.0 * k) };
    let r = Rect::new((screen_width() - w) / 2.0, (screen_height() - h) / 2.0, w, h);
    draw_rectangle(0.0, 0.0, screen_width(), screen_height(), Color::new(0.0, 0.0, 0.0, 0.35));
    let (title, lines) = match &status {
        Status::Found(r) => (
            tr("A new version of Razdor"),
            trf!("Razdor {new} is out (you have {old}). Update now? It downloads while you play and starts the next time you open the game. What changed:", new = r.version, old = update::current()),
        ),
        Status::Installed(r) => (tr("Update installed"), trf!("Razdor {v} is installed. It starts the next time you open the game.", v = r.version)),
        Status::InstallFailed(r, why) => (tr("Update failed"), trf!("Razdor {v} could not be installed: {why}", v = r.version, why)),
        _ => return false,
    };
    let (inner, closed) = chrome::window(r, title, chrome::Skin::Marble, true);
    let wrapped = wrap(&lines, inner.w - 48.0 * k, 14.0 * k);
    for (i, line) in wrapped.iter().take(5).enumerate() {
        chrome::shadow_text(line, inner.x + 24.0 * k, inner.y + 34.0 * k + i as f32 * 20.0 * k, 14.0 * k, chrome::CREAM);
    }
    if let (true, Status::Found(rel)) = (offer, &status) {
        let top = inner.y + 34.0 * k + wrapped.len().min(5) as f32 * 20.0 * k;
        let bottom = inner.y + inner.h - 56.0 * k;
        notes(Rect::new(inner.x + 24.0 * k, top, inner.w - 48.0 * k, bottom - top), &rel.notes);
    }
    let close = closed || key(KeyCode::Escape);
    match status {
        Status::Found(rel) => {
            let picked = buttons(inner, &[tr("Update"), tr("Always update"), tr("Later")]);
            if picked == Some(0) || picked == Some(1) || (picked.is_none() && key(KeyCode::Enter)) {
                // "Always update": from now on without this window.
                if picked == Some(1) {
                    *mode = Mode::Always;
                }
                ASKED.with(|a| a.set(true));
                TOLD.with(|t| t.set(false));
                update::install(rel);
            } else if picked == Some(2) || close {
                LATER.with(|l| l.set(true));
            }
        }
        Status::Installed(_) => {
            let labels: &[&str] = if on_title { &[tr("Restart now"), tr("OK")] } else { &[tr("OK")] };
            let picked = buttons(inner, labels);
            if on_title && picked == Some(0) {
                update::request_restart();
                return true;
            }
            if picked.is_some() || close || key(KeyCode::Enter) {
                TOLD.with(|t| t.set(true));
            }
        }
        Status::InstallFailed(rel, _) => match buttons(inner, &[tr("Open the release page"), tr("OK")]) {
            Some(0) => update::open_in_browser(&rel.page),
            Some(_) => TOLD.with(|t| t.set(true)),
            None if close || key(KeyCode::Enter) => TOLD.with(|t| t.set(true)),
            None => {}
        },
        _ => {}
    }
    // The screen below takes none of this frame's input.
    swallow_input();
    false
}
