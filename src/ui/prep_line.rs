//! The first start after an update converts the game folder, and a mod's build waits for the conversion
//! (`hub::mods::prepare_mod`). Then the install is read on a thread, and the window keeps running: it shows
//! the main menu drawn from the bundled title art (`ui::menu_art`, as the no-install first screen does) under
//! a 10% overlay, with a dark plate, the line «Подготовка файлов игры…» and the conversion's progress bar.
//! The menu takes no picks meanwhile. When the install is read, `App::adopt_started_install` replaces the
//! art (`announce_install`) as the mod switch does. A start with no wait is unchanged (`Start::Ready`).

use super::dt_art::DtArt;
use super::widgets;
use macroquad::prelude::*;
use razdor::dt::install::DtInstall;
use razdor::dt::DtError;
use razdor::i18n::tr;
use std::sync::mpsc::{Receiver, TryRecvError};

/// The line's size on the desktop: the px the other windows' text starts at (`display::text_for`'s base).
const DESKTOP_PX: f32 = 18.0;
/// The line's size on a phone, in sp (font px per dp): above the phones' floor of 12 sp (`mobile-ui`).
const PHONE_SP: f32 = 14.0;
/// The progress bar's height on a phone, in dp (at least 6 dp, the touch rule's bar height).
const PHONE_BAR_DP: f32 = 8.0;
/// The progress bar's width on a phone, as a share of the screen's width.
const PHONE_BAR_SHARE: f32 = 0.6;

/// The start's install, as `(install, job)`: [`razdor::dt::install::begin_load`] decides whether the load
/// waits for a conversion. No wait: the install is read now, as it always was (`DtArt::from_env`), and there
/// is no job. A wait: no install yet (the menu is drawn from the title art), and the job reads it on a thread
/// for [`wait`].
pub fn start() -> (Option<DtArt>, Option<Receiver<Result<DtInstall, DtError>>>) {
    if !razdor::dt::install::begin_load() {
        return (DtArt::from_env(), None);
    }
    let (tx, rx) = std::sync::mpsc::channel();
    let spawned = std::thread::Builder::new().name("install-load".into()).spawn(move || {
        let _ = tx.send(DtInstall::from_env());
    });
    // No thread: the install is read here (the thread never ran, so nothing was taken).
    match spawned {
        Ok(_) => (None, Some(rx)),
        Err(_) => (DtArt::from_env(), None),
    }
}

/// Waits for the install a [`Start::Waiting`] job reads, drawing the menu and the line each frame. Then the
/// install is adopted by the app. A reader that failed or ended without one leaves the app as it is.
pub async fn wait(app: &mut super::App, job: Receiver<Result<DtInstall, DtError>>) {
    loop {
        match job.try_recv() {
            Ok(Ok(install)) => {
                app.adopt_started_install(install);
                return;
            }
            Ok(Err(e)) => {
                razdor::diag!("Discord Times install not usable, using placeholders: {e}");
                return;
            }
            Err(TryRecvError::Disconnected) => return,
            Err(TryRecvError::Empty) => {}
        }
        super::frame_state::refresh();
        clear_background(BLACK);
        app.draw_menu_while_waiting();
        // The menu seen through a 10% overlay: the line and the bar are read on a dark plate.
        draw_rectangle(0.0, 0.0, screen_width(), screen_height(), Color::new(0.0, 0.0, 0.0, 0.10));
        draw_line_and_bar();
        next_frame().await;
    }
}

/// The line and the bar, centred on a dark plate over the menu.
fn draw_line_and_bar() {
    let (w, h) = (screen_width(), screen_height());
    let cx = w / 2.0;
    let cy = h / 2.0;
    let phone = super::display::startup_dp_px();
    let size = match phone {
        Some(dp) => PHONE_SP * dp,
        None => DESKTOP_PX,
    };
    let (bar_w, bar_h) = match phone {
        Some(dp) => (w * PHONE_BAR_SHARE, PHONE_BAR_DP * dp),
        None => (360.0 * super::chrome::k(), 14.0 * super::chrome::k()),
    };
    let line = tr("Preparing the game files…");
    let text_w = widgets::measure(line, size).width;
    let baseline = cy - size * 0.3;
    let bar_top = cy + size * 0.35;
    let pad = size * 0.5;
    let plate_w = text_w.max(bar_w) + 2.0 * pad;
    let plate_top = baseline - size * 0.8 - pad;
    let plate_bottom = bar_top + bar_h + pad;
    draw_rectangle(cx - plate_w / 2.0, plate_top, plate_w, plate_bottom - plate_top, Color::new(0.0, 0.0, 0.0, 0.6));
    widgets::text_centered(line, cx, baseline, size, widgets::INK);
    let share = razdor::dt::install::base_dir()
        .and_then(|base| razdor::dt::stored::conversion_progress(&base))
        .map_or(0.0, |(done, total)| if total == 0 { 0.0 } else { done as f32 / total as f32 });
    super::map_import::draw_bar(Rect::new(cx - bar_w / 2.0, bar_top, bar_w, bar_h), share);
}
