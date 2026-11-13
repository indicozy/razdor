//! The windows of `razdor::update`: the offer of a newer release ("Ask"), and once the
//! player chose to update, how it went. They open only at a calm moment (the title screen,
//! the settings, the world map with nothing else on it), never in a battle or over a
//! dialog. A check that fails or finds nothing shows nothing, unless the player asked for it
//! ("Check now" in the advanced settings, which shows [`status_label`]).

use razdor::local;
use std::cell::Cell;

use macroquad::prelude::*;

use razdor::i18n::tr;
use razdor::trf;
use razdor::update::{self, Mode, Note, Release, Status};

use super::audio::{cue, Cue};
use super::chrome;
use super::widgets::*;

local! {
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
/// runs on its own thread), "Always" puts a release found in place, and the system installer's
/// report on a handed-over update is taken (a cancel brings the offer back).
pub fn tick(mode: Mode) {
    if !STARTED.with(|s| s.replace(true)) && mode != Mode::Off && update::enabled() && !super::clock::is_snapshot() {
        update::check();
    }
    update::poll_installer();
    if let (Mode::Always, Status::Found(r)) = (update::effective(mode), update::status()) {
        update::install(r);
    }
}

/// "Check now": a check whose release is offered whatever the mode.
pub fn check_by_hand() {
    // A snapshot never asks GitHub: its screens show the same state on every run.
    if super::clock::is_snapshot() {
        return;
    }
    BY_HAND.with(|b| b.set(true));
    LATER.with(|l| l.set(false));
    update::check();
}

/// The update window wants to be on screen, at a calm moment.
pub fn wanted(mode: Mode, calm: bool) -> bool {
    let mode = update::effective(mode);
    calm && match update::status() {
        Status::Found(_) => mode != Mode::Always && (mode == Mode::Ask || BY_HAND.with(|b| b.get())) && !LATER.with(|l| l.get()),
        Status::Installed(_) | Status::Handed(_) | Status::InstallFailed(..) => ASKED.with(|a| a.get()) && !TOLD.with(|t| t.get()),
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
        asset: String::new(),
        member: None,
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

/// The version a screen shows for this program: its own, or in a snapshot a fixed placeholder,
/// so a version bump changes no picture of the regression check (tools/ui-regress).
pub fn shown_version(version: &str, snapshot: bool) -> String {
    if snapshot {
        SNAPSHOT_VERSION.into()
    } else {
        version.into()
    }
}

/// The placeholder a snapshot shows for the program's version.
pub const SNAPSHOT_VERSION: &str = "0.0.0";

/// The state for the advanced settings, next to "Check now".
pub fn status_label() -> String {
    match update::status() {
        Status::Idle => String::new(),
        Status::Checking => tr("Checking…").into(),
        Status::UpToDate => trf!("Up to date ({v})", v = shown_version(&update::current().to_string(), super::clock::is_snapshot())),
        Status::CheckFailed(_) => tr("Could not check").into(),
        Status::Found(r) => trf!("Razdor {v} is out", v = r.version),
        Status::Downloading(r) => trf!("Downloading {v}…", v = r.version),
        Status::Installed(r) => trf!("{v} starts next time", v = r.version),
        Status::Handed(r) => trf!("Razdor {v} waits for its install", v = r.version),
        Status::InstallFailed(..) => tr("Could not update").into(),
    }
}

/// The offer's text: what the release is and what the player does about it, per system.
fn offer_text(r: &Release) -> String {
    let old = shown_version(&update::current().to_string(), super::clock::is_snapshot());
    if update::opens_page() {
        trf!("Razdor {new} is out (you have {old}). Its page has the file: install it with AltStore or Sideloadly. What changed:", new = r.version, old = old)
    } else if razdor::platform::MOBILE {
        trf!("Razdor {new} is out (you have {old}). Update now? It downloads and is checked here, then Android asks you to confirm the install. What changed:", new = r.version, old = old)
    } else {
        trf!("Razdor {new} is out (you have {old}). Update now? It downloads while you play and starts the next time you open the game. What changed:", new = r.version, old = old)
    }
}

/// The title screen's line when an update is in place (an "Always" update says nothing
/// else).
pub fn title_line() {
    if let Status::Installed(r) = update::status() {
        let k = chrome::k();
        let line = trf!("Razdor {v} is installed and starts next time", v = r.version);
        text_centered(&line, screen_width() / 2.0, super::main_menu::notice_y(k), 16.0 * k, chrome::GOLD);
    }
}

/// The buttons' rectangles in `inner`, in label order: one row at the bottom right. Desktop:
/// Razdor's own size, grown by `g`. A phone: 40 dp at Normal (about 48 at the phone's default
/// text size), each button narrower only when the row does not fit.
fn button_rects(inner: Rect, n: usize, k: f32, g: f32, phone: bool) -> Vec<Rect> {
    let mut out = Vec::with_capacity(n);
    let (w, h, gap) = if phone {
        let gap = 10.0 * k;
        let h = 40.0 * k * g;
        let w = (150.0 * k * g).min((inner.w - 48.0 * k - n.saturating_sub(1) as f32 * gap) / n.max(1) as f32);
        (w, h, gap)
    } else {
        (150.0 * k * g, 28.0 * k * g, 12.0 * k)
    };
    let mut x = inner.x + inner.w - 24.0 * k - n as f32 * (w + gap) + gap;
    let y = button_y(inner, k, g, phone);
    for _ in 0..n {
        out.push(Rect::new(x, y, w, h));
        x += w + gap;
    }
    out
}

/// The top edge of the buttons' row: its bottom 16 dp above the window's bottom edge.
fn button_y(inner: Rect, k: f32, g: f32, phone: bool) -> f32 {
    let h = if phone { 40.0 * k * g } else { 28.0 * k * g };
    inner.y + inner.h - 16.0 * k - h
}

/// The list of changes' rectangle in `inner`, from `top` down to 12 dp above the buttons' row
/// (phone), or 56 dp above the window's bottom (desktop, the original's place).
fn notes_rect(inner: Rect, top: f32, k: f32, g: f32, phone: bool) -> Rect {
    let x = inner.x + 24.0 * k;
    let bottom = if phone { button_y(inner, k, g, true) - 12.0 * k } else { inner.y + inner.h - 56.0 * k - 28.0 * k * (g - 1.0) };
    Rect::new(x, top, inner.w - 48.0 * k, bottom - top)
}

/// A row of buttons (see [`button_rects`]); the index of the one clicked.
fn buttons(inner: Rect, labels: &[&str], phone: bool) -> Option<usize> {
    let k = chrome::k();
    // The buttons grow with the text size (Razdor's own, the owner's decision of 2026-10-31): 28 px
    // at Normal, their bottom edge where it is, so they grow up (`button_grow`).
    let g = super::display::button_grow();
    let rects = button_rects(inner, labels.len(), k, g, phone);
    let mut picked = None;
    super::dt_font::legacy_metrics_if(phone, || {
        for (i, (label, r)) in labels.iter().zip(rects).enumerate() {
            let (_, down, pressed) = super::widgets::button_state(r, true);
            chrome::marble_button(r, label, true, down);
            if pressed {
                cue(Cue::Button);
                picked = Some(i);
            }
        }
    });
    picked
}

/// How a list of changes is drawn: the desktop's sizes (in units of `chrome::k`), or a phone's
/// (larger, with a wider scrollbar and the spacing to match).
#[derive(Clone, Copy)]
struct NotesStyle {
    body: f32,
    line: f32,
    release: f32,
    section: f32,
    pad: f32,
    bullet: f32,
    /// The scrollbar: its left edge this far from the list's right edge, and its width.
    scroll_x: f32,
    scroll_w: f32,
    /// What the text keeps clear of the list's right edge, beyond the padding.
    cut: f32,
    /// The space above a release, above a section, and above the first line of an item.
    gap_release: f32,
    gap_section: f32,
    gap_item: f32,
}

impl NotesStyle {
    fn desktop(k: f32) -> Self {
        NotesStyle {
            body: 13.0 * k,
            line: 18.0 * k,
            release: 16.0 * k,
            section: 14.0 * k,
            pad: 12.0 * k,
            bullet: 16.0 * k,
            scroll_x: 15.0 * k,
            scroll_w: 12.0 * k,
            cut: 6.0 * k,
            gap_release: 10.0 * k,
            gap_section: 4.0 * k,
            gap_item: 2.0 * k,
        }
    }

    fn phone(k: f32) -> Self {
        NotesStyle {
            body: 14.0 * k,
            line: 20.0 * k,
            release: 17.0 * k,
            section: 15.0 * k,
            pad: 12.0 * k,
            bullet: 16.0 * k,
            scroll_x: 23.0 * k,
            scroll_w: 20.0 * k,
            cut: 24.0 * k,
            gap_release: 10.0 * k,
            gap_section: 4.0 * k,
            gap_item: 2.0 * k,
        }
    }
}

/// The list of changes in `r`, scrolled by the wheel, the arrow keys and the scrollbar.
fn notes(r: Rect, notes: &[Note], st: NotesStyle) {
    let k = chrome::k();
    draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.0, 0.0, 0.0, 0.35));
    chrome::silver_frame(r, 1.5);
    let (body, line, bullet) = (st.body, st.line, st.bullet);
    // The text keeps clear of the scrollbar.
    let room = r.w - 2.0 * st.pad - st.cut;
    // Each row: its text, where it starts, its colour, its size, the space above it.
    let mut rows: Vec<(String, f32, Color, f32, f32)> = Vec::new();
    for n in notes {
        match n {
            Note::Release(v) => rows.push((trf!("Razdor {v}", v), 0.0, chrome::GOLD, st.release, if rows.is_empty() { 0.0 } else { st.gap_release })),
            Note::Section(s) => rows.push((s.clone(), 0.0, chrome::CREAM, st.section, st.gap_section)),
            Note::Item(s) => {
                for (i, l) in wrap(s, room - bullet, body).into_iter().enumerate() {
                    let l = if i == 0 { format!("•  {l}") } else { l };
                    rows.push((l, if i == 0 { 0.0 } else { bullet }, WHITE, body, if i == 0 { st.gap_item } else { 0.0 }));
                }
            }
            Note::Text(s) => rows.extend(wrap(s, room, body).into_iter().map(|l| (l, 0.0, WHITE, body, 0.0))),
        }
    }
    let total: f32 = rows.iter().map(|(_, _, _, s, gap)| gap + s.max(line)).sum::<f32>() + 2.0 * st.pad;
    let most = (total - r.h).max(0.0);
    let mut scroll = SCROLL.with(|s| s.get());
    swipe_rows(r, 3.0 * line);
    if r.contains(pointer().into()) {
        scroll -= wheel() * 3.0 * line;
    }
    if key(KeyCode::Down) {
        scroll += 3.0 * line;
    }
    if key(KeyCode::Up) {
        scroll -= 3.0 * line;
    }
    let mut scroll = scroll.clamp(0.0, most);
    // Razdor's scrollbar on the right (a touch screen has no wheel).
    scrollbar_px(Rect::new(r.x + r.w - st.scroll_x, r.y + 3.0 * k, st.scroll_w, r.h - 6.0 * k), &mut scroll, most, r.h, line);
    SCROLL.with(|s| s.set(scroll));
    let _clip = Clip::new(r);
    let mut y = r.y + st.pad - scroll;
    for (text, indent, color, size, gap) in rows {
        y += gap + size.max(line);
        if y > r.y && y - size < r.y + r.h {
            chrome::shadow_text(&text, r.x + st.pad + indent, y - 4.0 * k, size, color);
        }
    }
}

/// Draws the update window over the screen and takes its clicks and keys. `on_title`: the
/// title screen or its settings, where "Restart now" loses no game. True when the player
/// asked to restart into the new version (the caller quits; `update::restart` starts it).
/// "Always update" sets the settings' `mode` to [`Mode::Always`] and updates now.
pub fn frame(on_title: bool, mode: &mut Mode) -> bool {
    let status = update::status();
    let offer = matches!(&status, Status::Found(r) if !r.notes.is_empty());
    draw_rectangle(0.0, 0.0, screen_width(), screen_height(), Color::new(0.0, 0.0, 0.0, 0.35));
    let (title, lines) = match &status {
        Status::Found(r) => (tr("A new version of Razdor"), offer_text(r)),
        Status::Installed(r) => (tr("Update installed"), trf!("Razdor {v} is installed. It starts the next time you open the game.", v = r.version)),
        Status::Handed(r) => (
            tr("Confirm the update"),
            trf!("Razdor {v} waits for you to confirm its install on the screen. If Android first asks to allow installs from Razdor, allow them: the update is offered again at the next start.", v = r.version),
        ),
        Status::InstallFailed(r, why) => (tr("Update failed"), trf!("Razdor {v} could not be installed: {why}", v = r.version, why)),
        _ => return false,
    };
    // A phone: the window's own scale is one dp, so the window, its text and its buttons read
    // `chrome::k` in dp; the desktop keeps the interface scale.
    let phone = super::display::dp_px();
    let on_phone = phone.is_some();
    let _scale = phone.map(|dp| super::display::enter_window_scale(Some(dp)));
    let k = chrome::k();
    // The window takes the buttons' extra height and width (`button_grow`: up to three buttons of
    // 150 px more each), its text and list keep their room.
    let g = super::display::button_grow();
    let grow = 28.0 * k * (g - 1.0);
    let wide = 3.0 * 150.0 * k * (g - 1.0);
    // Only a grown window is kept on the screen's width (at Normal it is as it always was).
    let fit_w = |w: f32| if wide > 0.0 { w.min(screen_width() - 16.0 * k) } else { w };
    // A phone's usable screen (less its safe area and the bottom bar): the window is the
    // desktop's size in dp, fitted to it and centred.
    let avail = phone.map(|dp| {
        let (sw, sh) = (screen_width(), screen_height());
        let safe = super::display::safe_area();
        let m = 6.0 * dp;
        Rect::new(safe.left + m, safe.top + m, sw - safe.left - safe.right - 2.0 * m, sh - safe.top - chrome::bar_height() - 2.0 * m)
    });
    // The intro text: a phone's is a little larger, and may run to eight lines (the desktop's five).
    let (size, side, cap) = if on_phone { (15.0 * k, 24.0 * k, 8) } else { (14.0 * k, 24.0 * k, 5) };
    let r = match avail {
        Some(avail) => {
            let (w, h) = if offer {
                ((680.0 * k).min(avail.w), (520.0 * k).min(avail.h))
            } else {
                let w = (600.0 * k).min(avail.w);
                // The desktop's 190 dp, plus a line step for each intro line past the first, and
                // the taller buttons.
                let lines_n = super::dt_font::legacy_metrics_if(true, || wrap(&lines, w - 80.0 * k, size)).len().max(1) as f32;
                (w, (190.0 * k + (lines_n - 1.0) * 21.0 * k + 12.0 * k * g).min(avail.h))
            };
            Rect::new(avail.x + (avail.w - w) / 2.0, avail.y + (avail.h - h) / 2.0, w, h)
        }
        None => {
            let (w, h) = if offer {
                (fit_w(680.0 * k + wide), (520.0 * k + grow).min(screen_height() - 16.0 * k))
            } else {
                (fit_w(600.0 * k + wide), 190.0 * k + grow)
            };
            Rect::new((screen_width() - w) / 2.0, (screen_height() - h) / 2.0, w, h)
        }
    };
    let (inner, closed) = chrome::window(r, title, chrome::Skin::Marble, true);
    // The intro's lines: the first baseline 34 dp under the window's top, as on the desktop, then
    // one line step each (21 dp on a phone, 20 dp on the desktop).
    let line_y = |i: usize| {
        if on_phone {
            inner.y + 34.0 * k + i as f32 * 21.0 * k
        } else {
            inner.y + 34.0 * k + i as f32 * 20.0 * k
        }
    };
    let shown = super::dt_font::legacy_metrics_if(on_phone, || {
        let wrapped = wrap(&lines, inner.w - 2.0 * side, size);
        for (i, line) in wrapped.iter().take(cap).enumerate() {
            chrome::shadow_text(line, inner.x + side, line_y(i), size, chrome::CREAM);
        }
        wrapped.len().min(cap)
    });
    if let (true, Status::Found(rel)) = (offer, &status) {
        let top = if on_phone { inner.y + 34.0 * k + shown as f32 * 21.0 * k } else { inner.y + 34.0 * k + shown as f32 * 20.0 * k };
        let list = notes_rect(inner, top, k, g, on_phone);
        let style = if on_phone { NotesStyle::phone(k) } else { NotesStyle::desktop(k) };
        super::dt_font::legacy_metrics_if(on_phone, || notes(list, &rel.notes, style));
    }
    let close = closed || key(KeyCode::Escape);
    match status {
        Status::Found(rel) if update::opens_page() => {
            // iOS installs nothing: the release's page opens in the browser, where the IPA is.
            match buttons(inner, &[tr("Open the release page"), tr("Later")], on_phone) {
                Some(0) => {
                    super::main_menu::open_link(&rel.page);
                    LATER.with(|l| l.set(true));
                }
                Some(_) => LATER.with(|l| l.set(true)),
                None if close => LATER.with(|l| l.set(true)),
                None => {}
            }
        }
        Status::Found(rel) => {
            // Desktop: "Always update" too (from now on without this window). A phone has no
            // "Always" (the system's installer asks), so its buttons are "Update" and "Later".
            let desktop = !razdor::platform::MOBILE;
            let labels: Vec<&str> = if desktop { vec![tr("Update"), tr("Always update"), tr("Later")] } else { vec![tr("Update"), tr("Later")] };
            let later = labels.len() - 1;
            let picked = buttons(inner, &labels, on_phone);
            let always = desktop && picked == Some(1);
            if picked == Some(0) || always || (picked.is_none() && key(KeyCode::Enter)) {
                if always {
                    *mode = Mode::Always;
                }
                ASKED.with(|a| a.set(true));
                TOLD.with(|t| t.set(false));
                update::install(rel);
            } else if picked == Some(later) || close {
                LATER.with(|l| l.set(true));
            }
        }
        Status::Handed(_) => {
            if buttons(inner, &[tr("OK")], on_phone).is_some() || close || key(KeyCode::Enter) {
                TOLD.with(|t| t.set(true));
            }
        }
        Status::Installed(_) => {
            let labels: &[&str] = if on_title { &[tr("Restart now"), tr("OK")] } else { &[tr("OK")] };
            let picked = buttons(inner, labels, on_phone);
            if on_title && picked == Some(0) {
                update::request_restart();
                return true;
            }
            if picked.is_some() || close || key(KeyCode::Enter) {
                TOLD.with(|t| t.set(true));
            }
        }
        Status::InstallFailed(rel, _) => match buttons(inner, &[tr("Open the release page"), tr("OK")], on_phone) {
            Some(0) => super::main_menu::open_link(&rel.page),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_snapshot_shows_the_placeholder_version_and_a_live_run_its_own() {
        assert_eq!(shown_version("0.4.40", true), "0.0.0");
        assert_eq!(shown_version("0.4.40", false), "0.4.40");
        assert_eq!(shown_version("9.9.9", true), SNAPSHOT_VERSION);
    }

    /// `r` lies inside `outer` (a pixel of slack for rounding).
    fn within(outer: Rect, r: Rect) -> bool {
        r.x >= outer.x - 1.0 && r.y >= outer.y - 1.0 && r.x + r.w <= outer.x + outer.w + 1.0 && r.y + r.h <= outer.y + outer.h + 1.0
    }

    #[test]
    fn the_desktop_buttons_are_the_row_they_always_were() {
        // 150 px wide, 28 high, 12 apart, the row right-aligned 24 px in and 16 px up.
        let inner = Rect::new(100.0, 50.0, 600.0, 300.0);
        let rs = button_rects(inner, 3, 1.0, 1.0, false);
        assert_eq!(rs.iter().map(|b| b.x).collect::<Vec<_>>(), vec![202.0, 364.0, 526.0]);
        assert!(rs.iter().all(|b| b.y == 306.0 && b.w == 150.0 && b.h == 28.0));
    }

    /// The two rectangles share some area.
    fn overlaps(a: Rect, b: Rect) -> bool {
        a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
    }

    #[test]
    fn a_phone_keeps_one_row_of_buttons_inside_the_window_and_the_list_above_it() {
        // A phone's window is 360 dp wide: three buttons fit in the row.
        let k = 2.75;
        let inner = Rect::new(0.0, 0.0, 360.0 * k, 300.0 * k);
        let list = notes_rect(inner, 60.0 * k, k, 1.0, true);
        assert!(within(inner, list), "the list is outside the window: {list:?}");
        for n in [2usize, 3] {
            let rs = button_rects(inner, n, k, 1.0, true);
            assert_eq!(rs.len(), n);
            for (i, b) in rs.iter().enumerate() {
                assert!(within(inner, *b), "button {i} outside the window: {b:?}");
                assert_eq!(b.y, rs[0].y, "the buttons are not in one row");
                assert!(b.h >= 40.0 * k, "button {i} is {} px, under 40 dp", b.h);
                assert!(!overlaps(list, *b), "button {i} overlaps the list");
                if i > 0 {
                    assert!(rs[i - 1].x + rs[i - 1].w <= b.x, "buttons {} and {i} overlap", i - 1);
                }
            }
            let last = rs.last().unwrap();
            assert!((last.x + last.w - (inner.x + inner.w - 24.0 * k)).abs() < 0.01, "the row is not right-aligned");
            assert!(list.y + list.h <= rs[0].y - 11.9 * k, "the list does not stop above the buttons");
        }
    }
}
